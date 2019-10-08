use std::*;
use getopts::Options;

use simple_logging;
use log::LevelFilter;



mod portforwarder;
use portforwarder::{forward, get_ipv4_socket_addr};

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt("s", "src", "where to listen on (default 0.0.0.0:815", "HOST:PORT");
    opts.optopt("d", "dst", "where to forward to (default zm.tolao.de:815", "HOST:PORT");
    opts.optflag("h", "help", "print this help");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };
    if matches.opt_present("h") {
        print_usage(&program, opts);
        return Ok(());
    }

    simple_logging::log_to_stderr(LevelFilter::Info);
    let src_str = matches.opt_str("src").unwrap_or_else(|| "0.0.0.0:815".to_string());
    let dst_str = matches.opt_str("dst").unwrap_or_else(|| "zm.tolao.de:815".to_string());
    let src = get_ipv4_socket_addr(&src_str)?;
    let dst = get_ipv4_socket_addr(&dst_str)?;

    loop {
        if let Err(e) = forward(src, dst, None) {
            println!("Forwarding failed: {}", e);
        }
    }
}
