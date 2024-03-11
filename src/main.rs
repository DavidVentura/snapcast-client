use serde::Serialize;
use std::io::prelude::*;
use std::net::TcpStream;

#[derive(Copy, Clone)]
pub struct TimeVal {
    pub sec: i32,
    pub usec: i32,
}

#[repr(u16)]
#[derive(Copy, Clone)]
pub enum MessageType {
    Base = 0,
    CodecHeader = 1,
    WireChunk = 2,
    ServerSettings = 3,
    Time = 4,
    Hello = 5,
    StreamTags = 6,
    ClientInfo = 7,
}

struct Base<'a> {
    mtype: MessageType,
    id: u16,
    refers_to: u16,
    sent_tv: TimeVal,
    received_tv: TimeVal,
    size: u32,
    payload: &'a [u8],
}

pub trait Message {
    fn as_buf(&self) -> Vec<u8>;
}
impl<'a> Base<'a> {
    const BASE_SIZE: usize = 26;
}
impl<'a> Message for Base<'a> {
    fn as_buf(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.payload.len() + Base::BASE_SIZE);

        buf.extend(u16::to_le_bytes(self.mtype as u16));
        buf.extend(u16::to_le_bytes(self.id));
        buf.extend(u16::to_le_bytes(self.refers_to));
        buf.extend(i32::to_le_bytes(self.sent_tv.sec));
        buf.extend(i32::to_le_bytes(self.sent_tv.usec));
        buf.extend(i32::to_le_bytes(self.received_tv.sec));
        buf.extend(i32::to_le_bytes(self.received_tv.usec));
        buf.extend(u32::to_le_bytes(self.size + 4)); // wtf?
        buf.extend(u32::to_le_bytes(self.size));
        buf.extend(self.payload);
        buf
    }
}
impl<'a> Message for ClientHello<'a> {
    fn as_buf(&self) -> Vec<u8> {
        let p_str = serde_json::to_string(&self).unwrap();
        let payload = p_str.as_bytes();

        Base {
            mtype: MessageType::Hello,
            id: 0,
            refers_to: 0,
            sent_tv: TimeVal { sec: 0, usec: 0 },
            received_tv: TimeVal { sec: 0, usec: 0 },
            size: payload.len() as u32,
            payload,
        }
        .as_buf()
    }
}

#[allow(non_snake_case)]
#[derive(Serialize)]
struct ClientHello<'a> {
    MAC: &'a str,
    HostName: &'a str,
    Version: &'a str,
    ClientName: &'a str,
    OS: &'a str,
    Arch: &'a str,
    Instance: u8,
    ID: &'a str,
    SnapStreamProtocolVersion: u8,
}
fn main() {
    println!("Hello, world!");
    let ch = ClientHello {
        Arch: "x86_64",
        ClientName: "CoolClient",
        HostName: "framework",
        ID: "00:11:22:33:44:55",
        Instance: 1,
        MAC: "00:11:22:33:44:55",
        SnapStreamProtocolVersion: 2,
        Version: "0.17.1",
        OS: "an os",
    };
    let b = ch.as_buf();
    println!("{:?}", b);
    let mut s = TcpStream::connect("127.0.0.1:1704").unwrap();
    s.write(&b).unwrap();
}
