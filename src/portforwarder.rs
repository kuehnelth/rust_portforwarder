use std::*;
use std::io::{Read, Write};
#[cfg(windows)]
use wepoll_binding::{Epoll, EventFlag, Events};
#[cfg(not(windows))]
use fake_wepoll_binding::{Epoll, EventFlag, Events};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::collections::HashMap;
use multi_map::MultiMap;
use log::{debug, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{thread, time};

struct TcpConnection {
    src: TcpStream,
    dst_id: u64,
}

struct UdpConnection {
    src: UdpSocket,
    addr: SocketAddr,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Token(pub u64);

pub fn get_ipv4_socket_addr(input :&str) -> Result<SocketAddr, io::Error> {
    let addrs_iter = input.to_socket_addrs()?;
    for addr in addrs_iter {
        if addr.is_ipv4() { return Ok(addr); }
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "Can't resolve input to IPv4 socket address"))
}

pub fn forward(src: SocketAddr, dst: SocketAddr, abort: Option<&AtomicBool>) -> Result<(), io::Error> {
    const TCP_SERVER: u64 = 0;
    const UDP_SERVER: u64 = 1;
    let mut next_token = 2;
    let mut tcp_conns = HashMap::with_capacity(32);
    let mut udp_conns = MultiMap::with_capacity(32);

    let epoll = Epoll::new()?;

    let tcp_server = TcpListener::bind(src)?;
    epoll.register(&tcp_server, EventFlag::IN, TCP_SERVER)?;

    let udp_server = UdpSocket::bind(src)?;
    epoll.register(&udp_server, EventFlag::IN, UDP_SERVER)?;


    // Create storage for events
    let mut events = Events::with_capacity(1024);
    let mut buf = [0; 8192];

	let timeout = if abort.is_some() {
		Some(Duration::from_millis(100))
	} else {
		None
	};

    info!("Start forwarding from {} to {}", src, dst);

    loop {
        epoll.poll(&mut events, timeout)?;

        if let Some(a) = abort {
            if a.load(Ordering::Relaxed) {
                info!("stopping portforwarder!");
                break;
            }
        }

        for event in events.iter() {
            debug!("event {:?}", event.data());
            match event.data() {
                TCP_SERVER => {
                        match tcp_server.accept() {
                            Ok((stream1, from)) => {
                                info!("New TCP connection {:?}", from);
                                epoll.register(&stream1, EventFlag::IN, next_token)?;
                                next_token += 1;
                                let stream2 = TcpStream::connect(dst)?;
                                epoll.register(&stream2, EventFlag::IN, next_token)?;
                                next_token += 1;

                                let conn1 = TcpConnection{src: stream1, dst_id: next_token - 1};
                                let conn2 = TcpConnection{src: stream2, dst_id: next_token - 2};
                                tcp_conns.insert(next_token - 2, conn1);
                                tcp_conns.insert(next_token - 1, conn2);
                                thread::sleep(time::Duration::from_millis(20));

                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            }
                            Err(e) => {
                                warn!("error {:?}", e);
                                return Err(e);
                            }
                        }
                }

                UDP_SERVER => {
                    if let Ok((len, from)) = udp_server.recv_from(&mut buf) {
                        if !udp_conns.contains_key_alt(&from) {
                            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
                            debug!("read {} bytes udp from {:?}", len, from);
                            let dst_sock = UdpSocket::bind(&addr)?;

                            epoll.register(&dst_sock, EventFlag::IN, next_token)?;
                            let conn = UdpConnection{src: dst_sock, addr: from};
                            udp_conns.insert(next_token, from, conn);
                            next_token += 1;
                        }

                        if let Some(dst_conn) = udp_conns.get_alt(&from) {
                            let dst_sock = &dst_conn.src;
                            debug!("read {} bytes udp from {:?}", len, from);
                            dst_sock.send_to(&buf[..len], &dst)?;
                        }
                    }
                }
                port => {
                    let mut to_remove = None;
                    let mut send_to = None;
                    let mut send_len = 0;
                    let mut buf = [0; 8192];

                    if let Some(c) = tcp_conns.get_mut(&port) {
                        match c.src.read(&mut buf) {
                            Ok(0) => {
                                warn!("read {} bytes tcp from {:?}", 0, c.src.peer_addr()?);
                                to_remove = Some((port, c.dst_id));
                            }
                            Ok(len) => {
                                debug!("read {} bytes tcp from {:?}", len, c.src.peer_addr()?);
                                send_to = Some(c.dst_id);
                                send_len = len;
                            }
                            Err(e) => {
                                if e.kind() != io::ErrorKind::WouldBlock {
                                    warn!("TCP error: {}", e);
                                    to_remove = Some((port, c.dst_id));
                                }
                            }
                        }
                    }

                    if let Some(dst_id) = send_to {
                        if let Some(d) = tcp_conns.get_mut(&dst_id) {
                            if let Err(e) = d.src.write(&buf[..send_len]) {
                                warn!("TCP error {:?}", e);
                                to_remove = Some((port, dst_id));
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
                                debug!("read {} bytes udp from {:?}", len, from);
                                let _ = udp_server.send_to(&buf[..len], &c.addr);
                            }
                            Err(e) => {
                                warn!("UDP error {:?}", e);
                            }
                        }
                    }
                },

            }
        }


    }

    Ok(())
}
