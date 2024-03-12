mod proto;

use proto::{Base, Server};
use std::io::prelude::*;
use std::net::TcpStream;

fn main() {
    println!("Hello, world!");
    let srv = Server::new("11:22:33:44:55:66".into(), "framework".into());
    let b = srv.hello();
    let mut s = TcpStream::connect("127.0.0.1:1704").unwrap();
    s.write(&b).unwrap();
    loop {
        let mut buf = vec![0; 1500];
        let b = s.read(&mut buf).unwrap();
        println!("read bytes {b}");
        let b = Base::from(&buf[0..b]);
        println!("{:?}", b.decode());
    }
}
