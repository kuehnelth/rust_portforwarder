use mio::*;
use mio::net::{TcpListener, TcpStream, UdpSocket};
use std::net::{SocketAddr, ToSocketAddrs};
use std::collections::HashMap;
use multi_map::MultiMap;
use std::*;

struct TcpConnection {
    src: TcpStream,
    dst_id: usize,
}

struct UdpConnection {
    src: UdpSocket,
    addr: SocketAddr,
}

fn get_ipv4_socket_addr(input :&str) -> Result<SocketAddr, io::Error> {
    let addrs_iter = input.to_socket_addrs()?;
    for addr in addrs_iter {
        if addr.is_ipv4() { return Ok(addr); }
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "Can't resolve input to IPv4 socket address"))
}

fn forward(src: SocketAddr, dst: SocketAddr) -> Result<(), io::Error> {
    const TCP_SERVER: Token = Token(0);
    const UDP_SERVER: Token = Token(1);
    let mut next_token = 2;
    let mut tcp_conns = HashMap::with_capacity(32);
    let mut udp_conns = MultiMap::with_capacity(32);

    let poll = Poll::new()?;

    let tcp_server = TcpListener::bind(&src)?;
    poll.register(&tcp_server, TCP_SERVER, Ready::readable(),
                  PollOpt::edge())?;

    let udp_server = UdpSocket::bind(&src)?;
    poll.register(&udp_server, UDP_SERVER, Ready::readable(),
                  PollOpt::edge())?;


    // Create storage for events
    let mut events = Events::with_capacity(1024);
    let mut buf = [0; 8192];

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                TCP_SERVER => {
                    let (stream1, _) = tcp_server.accept().unwrap();
                    poll.register(&stream1, Token(next_token), Ready::readable(),
                                  PollOpt::edge())?;
                    next_token += 1;
                    let stream2 = TcpStream::connect(&dst).unwrap();
                    poll.register(&stream2, Token(next_token), Ready::readable(),
                                  PollOpt::edge())?;
                    next_token += 1;

                    let conn1 = TcpConnection{src: stream1, dst_id: next_token - 1};
                    let conn2 = TcpConnection{src: stream2, dst_id: next_token - 2};
                    tcp_conns.insert(next_token - 2, conn1);
                    tcp_conns.insert(next_token - 1, conn2);
                }
                UDP_SERVER => {
                    if let Ok((len, from)) = udp_server.recv_from(&mut buf) {
                        if !udp_conns.contains_key_alt(&from) {
                            let addr = "0.0.0.0:0".parse().unwrap();
                            let dst_sock = UdpSocket::bind(&addr).unwrap();

                            poll.register(&dst_sock, Token(next_token), Ready::readable(),
                                          PollOpt::edge())?;
                            let conn = UdpConnection{src: dst_sock, addr: from};
                            udp_conns.insert(next_token, from, conn);
                            next_token += 1;
                        }

                        if let Some(dst_conn) = udp_conns.get_alt(&from) {
                            let dst_sock = &dst_conn.src;
                            dst_sock.send_to(&buf[..len], &dst).unwrap();
                        }
                    }
                }
                Token(port) => {
                    if let Some(c) = tcp_conns.get(&port) {
                        let buffer_ref: &mut [u8] = &mut buf;
                        let mut buffers: [&mut IoVec; 1] = [buffer_ref.into()];
                        let len = c.src.read_bufs(&mut buffers).unwrap_or_default();
                        if len > 0 {
                            if let Some(d) = tcp_conns.get(&c.dst_id) {
                                let d_buffers: [&IoVec; 1] = [buf[..len].into()];
                                d.src.write_bufs(&d_buffers).unwrap();
                            }
                        }
                    }
                    if let Some(c) = udp_conns.get(&port) {
                        if let Ok((len, _)) = c.src.recv_from(&mut buf) {
                            udp_server.send_to(&buf[..len], &c.addr).unwrap();
                        }
                    }
                },
            }
        }
    }
}

fn main() -> Result<(), std::io::Error> {
    let src = get_ipv4_socket_addr("0.0.0.0:1815")?;
    let dst = get_ipv4_socket_addr("127.0.0.1:2815")?;

    forward(src, dst)
}
