use mio::*;
use mio::net::{TcpListener, TcpStream, UdpSocket};
use std::net::SocketAddr;
use std::collections::HashMap;
use multi_map::MultiMap;

struct TcpConnection {
    src: TcpStream,
    dst_id: usize,
}

struct UdpConnection {
    src: UdpSocket,
    addr: SocketAddr,
}

fn main() {
    const TCP_SERVER: Token = Token(0);
    const UDP_SERVER: Token = Token(1);
    let mut next_token = 2;
    let mut tcp_conns = HashMap::with_capacity(32);
    let mut udp_conns = MultiMap::with_capacity(32);

    let poll = Poll::new().unwrap();

    let addr = "0.0.0.0:1815".parse().unwrap();
    let addr2 = "127.0.0.1:2815".parse().unwrap();

    let tcp_server = TcpListener::bind(&addr).unwrap();
    poll.register(&tcp_server, TCP_SERVER, Ready::readable(),
                  PollOpt::edge()).unwrap();

    let udp_server = UdpSocket::bind(&addr).unwrap();
    poll.register(&udp_server, UDP_SERVER, Ready::readable(),
                  PollOpt::edge()).unwrap();


    // Create storage for events
    let mut events = Events::with_capacity(1024);
    let mut buf = [0; 8192];

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in events.iter() {
            match event.token() {
                TCP_SERVER => {
                    let (stream1, _) = tcp_server.accept().unwrap();
                    poll.register(&stream1, Token(next_token), Ready::readable(),
                                  PollOpt::edge()).unwrap();
                    next_token += 1;
                    let stream2 = TcpStream::connect(&addr2).unwrap();
                    poll.register(&stream2, Token(next_token), Ready::readable(),
                                  PollOpt::edge()).unwrap();
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
                                          PollOpt::edge()).unwrap();
                            let conn = UdpConnection{src: dst_sock, addr: from};
                            udp_conns.insert(next_token, from, conn);
                            next_token += 1;
                        }

                        if let Some(dst_conn) = udp_conns.get_alt(&from) {
                            let dst_sock = &dst_conn.src;
                            dst_sock.send_to(&buf[..len], &addr2).unwrap();
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
