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

impl From<u16> for MessageType {
    fn from(u: u16) -> MessageType {
        match u {
            0 => MessageType::Base,
            1 => MessageType::CodecHeader,
            2 => MessageType::WireChunk,
            3 => MessageType::ServerSettings,
            4 => MessageType::Time,
            5 => MessageType::Hello,
            6 => MessageType::StreamTags,
            7 => MessageType::ClientInfo,
        }
    }
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

fn slice_to_u16(s: &[u8]) -> u16 {
    u16::from_le_bytes([s[0], s[1]])
}
fn slice_to_u32(s: &[u8]) -> u32 {
    u32::from_le_bytes([s[0], s[1], s[2], s[3]])
}

impl<'a> From<&[u8]> for Base<'a> {
    fn from(buf: &[u8]) {
        let mtype: MessageType = slice_to_u16(&buf[0..2]).into();
        let id = slice_to_u16(&buf[2..4]);
        let refers_to = slice_to_u16(&buf[4..6]);
        let sent_s = slice_to_u32(&buf[6..10]);
        let sent_u = slice_to_u32(&buf[10..14]);
        let recv_s = slice_to_u32(&buf[14..18]);
        let recv_u = slice_to_u32(&buf[22..26]);
        let size = slice_to_u32(&buf[24..28]);
    }
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
        buf.extend(u32::to_le_bytes(self.size));
        buf.extend(self.payload);
        buf
    }
}
impl<'a> Message for ClientHello<'a> {
    fn as_buf(&self) -> Vec<u8> {
        let p_str = serde_json::to_string(&self).unwrap();
        let payload = p_str.as_bytes();
        let payload_len_buf = u32::to_le_bytes(payload.len() as u32).to_vec();
        payload_len_buf.extend_from_slice(payload);
        let payload = payload_len_buf;
        Base {
            mtype: MessageType::Hello,
            id: 0,
            refers_to: 0,
            sent_tv: TimeVal { sec: 0, usec: 0 },
            received_tv: TimeVal { sec: 0, usec: 0 },
            size: payload.len() as u32 + 4,
            payload: &payload,
        }
        .as_buf()
    }
}

struct ServerSettings {
    payload: String,
}

struct CodecHeader {
    codec: String,
    payload: Vec<u8>,
}

struct WireChunk<'a> {
    timestamp: TimeVal,
    payload: &'a [u8],
}
struct Time {
    latency: TimeVal,
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
    loop {
        let mut buf = vec![0; 1500];
        let b = s.read(&mut buf).unwrap();
        println!("read bytes {b}; got {buf:?}");
    }
}
