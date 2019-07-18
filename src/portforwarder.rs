use std::*;
use mio::*;
use mio::net::{TcpListener, TcpStream, UdpSocket};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::collections::HashMap;
use multi_map::MultiMap;
use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct TcpConnection {
    src: TcpStream,
    dst_id: usize,
    writable: bool,
}

struct UdpConnection {
    src: UdpSocket,
    addr: SocketAddr,
}

pub fn get_ipv4_socket_addr(input :&String) -> Result<SocketAddr, io::Error> {
    let addrs_iter = input.to_socket_addrs()?;
    for addr in addrs_iter {
        if addr.is_ipv4() { return Ok(addr); }
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "Can't resolve input to IPv4 socket address"))
}

pub fn forward(src: SocketAddr, dst: SocketAddr, abort: Option<&AtomicBool>) -> Result<(), io::Error> {
    const TCP_SERVER: Token = Token(0);
    const UDP_SERVER: Token = Token(1);
    let mut next_token = 2;
    let mut tcp_conns = HashMap::with_capacity(32);
    let mut udp_conns = MultiMap::with_capacity(32);

    let poll = Poll::new()?;

    let tcp_server = TcpListener::bind(&src)?;
    poll.register(&tcp_server, TCP_SERVER, Ready::readable(),
                  PollOpt::level())?;

    let udp_server = UdpSocket::bind(&src)?;
    poll.register(&udp_server, UDP_SERVER, Ready::readable(),
                  PollOpt::level())?;


    // Create storage for events
    let mut events = Events::with_capacity(1024);
    let mut buf = [0; 8192];

    let timeout;
    if abort.is_some() {
        timeout = Some(Duration::from_millis(100));
    } else {
        timeout = None;
    }

    info!("Start forwarding from {} to {}", src, dst);

    loop {
        poll.poll(&mut events, timeout)?;

        if let Some(a) = abort {
            if a.load(Ordering::Relaxed) {
                info!("stopping portforwarder!");
                break;
            }
        }

        for event in events.iter() {
            //info!("event {:?}", event);
            match event.token() {
                TCP_SERVER => {
                    /* connect happens async so we have to wait for a writable
                     * event to know that the connection is established.
                     * after that we will switch to listening for readable
                     */
                    let (stream1, from) = tcp_server.accept()?;
                    info!("New TCP connection {:?}", from);
                    poll.register(&stream1, Token(next_token), Ready::writable(),
                                  PollOpt::level())?;
                    next_token += 1;
                    let stream2 = TcpStream::connect(&dst)?;
                    poll.register(&stream2, Token(next_token), Ready::writable(),
                                  PollOpt::level())?;
                    next_token += 1;

                    let conn1 = TcpConnection{src: stream1, dst_id: next_token - 1, writable: false};
                    let conn2 = TcpConnection{src: stream2, dst_id: next_token - 2, writable: false};
                    tcp_conns.insert(next_token - 2, conn1);
                    tcp_conns.insert(next_token - 1, conn2);
                }
                UDP_SERVER => {
                    if let Ok((len, from)) = udp_server.recv_from(&mut buf) {
                        if !udp_conns.contains_key_alt(&from) {
                            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
                            info!("read {} bytes udp from {:?}", len, from);
                            let dst_sock = UdpSocket::bind(&addr)?;

                            poll.register(&dst_sock, Token(next_token), Ready::readable(),
                                          PollOpt::level())?;
                            let conn = UdpConnection{src: dst_sock, addr: from};
                            udp_conns.insert(next_token, from, conn);
                            next_token += 1;
                        }

                        if let Some(dst_conn) = udp_conns.get_alt(&from) {
                            let dst_sock = &dst_conn.src;
                            info!("read {} bytes udp from {:?}", len, from);
                            dst_sock.send_to(&buf[..len], &dst)?;
                        }
                    }
                }
                Token(port) => {
                    if event.readiness().is_writable() {
                        if let Some(c) = tcp_conns.get_mut(&port) {
                            poll.reregister(&c.src, Token(port), Ready::readable(),
                                            PollOpt::level())?;
                            c.writable = true;
                            continue;
                        }

                    }
                    let mut to_remove = None;
                    if let Some(c) = tcp_conns.get(&port) {
                        if let Some(d) = tcp_conns.get(&c.dst_id) {
                            if !d.writable {
                                continue;
                            }
                        }

                        let buffer_ref: &mut [u8] = &mut buf;
                        let mut buffers: [&mut IoVec; 1] = [buffer_ref.into()];
                        match c.src.read_bufs(&mut buffers) {
                            Ok(0) => {
                                warn!("read {} bytes tcp from {:?}", 0, c.src.peer_addr()?);
                                /*
                                if let Some(d) = tcp_conns.get(&c.dst_id) {
                                    let d_buffers: [&IoVec; 0] = [];
                                    d.src.write_bufs(&d_buffers)?;
                                }
                                */
                            }
                            Ok(len) => {
                                info!("read {} bytes tcp from {:?}", len, c.src.peer_addr()?);
                                if let Some(d) = tcp_conns.get(&c.dst_id) {
                                    let d_buffers: [&IoVec; 1] = [buf[..len].into()];
                                    d.src.write_bufs(&d_buffers)?;
                                }
                            }
                            Err(e) => {
                                if e.kind() != io::ErrorKind::WouldBlock {
                                    warn!("TCP error {:?}", e);
                                    to_remove = Some((port, c.dst_id));
                                }
                            }
                        }
                    }
                    if let Some((port1, port2)) = to_remove {
                        info!("Closing TCP connections {} and {}", port1, port2);
                        tcp_conns.remove(&port1);
                        tcp_conns.remove(&port2);
                    }

                    if let Some(c) = udp_conns.get(&port) {
                        match c.src.recv_from(&mut buf) {
                            Ok((len, from)) => {
                                info!("read {} bytes udp from {:?}", len, from);
                                let _ = udp_server.send_to(&buf[..len], &c.addr);
                            }
                            Err(e) => {
                                //if e.kind() != io::ErrorKind::WouldBlock {
                                    warn!("UDP error {:?}", e);
                                //}
                            }
                        }
                    }
                },
            }
        }
    }

    Ok(())
}
