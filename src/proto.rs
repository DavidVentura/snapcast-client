use std::io::Read;

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug)]
pub struct TimeVal {
    pub sec: i32,
    pub usec: i32,
}

impl From<&[u8]> for TimeVal {
    fn from(buf: &[u8]) -> TimeVal {
        TimeVal {
            sec: slice_to_i32(&buf[0..4]),
            usec: slice_to_i32(&buf[4..8]),
        }
    }
}
#[repr(u16)]
#[derive(Copy, Clone, Debug)]
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

#[derive(Debug)]
pub enum ServerMessage<'a> {
    ServerSettings(ServerSettings),
    CodecHeader(CodecHeader<'a>),
    WireChunk(WireChunk<'a>),
    Time(Time),
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
            _ => panic!("Illegal message type"),
        }
    }
}
#[derive(Debug)]
pub struct Base<'a> {
    mtype: MessageType,
    id: u16,
    refers_to: u16,
    sent_tv: TimeVal,
    received_tv: TimeVal,
    size: u32,
    payload: &'a [u8],
}

pub trait SerializeMessage {
    fn as_buf(&self) -> Vec<u8>;
}

fn slice_to_u16(s: &[u8]) -> u16 {
    u16::from_le_bytes([s[0], s[1]])
}
fn slice_to_i32(s: &[u8]) -> i32 {
    i32::from_le_bytes([s[0], s[1], s[2], s[3]])
}
fn slice_to_u32(s: &[u8]) -> u32 {
    u32::from_le_bytes([s[0], s[1], s[2], s[3]])
}
impl<'a> From<&'a [u8]> for CodecHeader<'a> {
    fn from(buf: &'a [u8]) -> CodecHeader<'a> {
        let size = slice_to_u32(&buf[0..4]) as usize;
        let codec_name_end = 4 + size;
        let codec = std::str::from_utf8(&buf[4..codec_name_end]).unwrap();

        let payload_len = slice_to_u32(&buf[codec_name_end..codec_name_end + 4]);
        let payload = &buf[codec_name_end + 4..codec_name_end + 4 + payload_len as usize];
        let metadata = match codec {
            "opus" => CodecMetadata::Opus(OpusMetadata::from(payload)),
            "flac" => CodecMetadata::Opaque(payload),
            "pcm" => CodecMetadata::Opaque(payload),
            "ogg" => CodecMetadata::Opaque(payload),
            _ => todo!("unsupported codec {}", codec),
        };
        CodecHeader { codec, metadata }
    }
}
impl<'a> From<&'a [u8]> for ServerSettings {
    fn from(buf: &'a [u8]) -> ServerSettings {
        let len = slice_to_u32(&buf[0..4]);
        let s = std::str::from_utf8(&buf[4..4 + len as usize]).expect("Bad UTF8 data");
        serde_json::from_str(s).unwrap()
    }
}

impl<'a> From<&'a [u8]> for WireChunk<'a> {
    fn from(buf: &'a [u8]) -> WireChunk<'a> {
        let size = slice_to_u32(&buf[8..12]);
        WireChunk {
            timestamp: TimeVal::from(&buf[0..8]),
            payload: &buf[12..12 + size as usize],
        }
    }
}
impl<'a> From<&'a [u8]> for Time {
    fn from(buf: &'a [u8]) -> Time {
        Time {
            latency: TimeVal::from(&buf[0..8]),
        }
    }
}
impl<'a> From<&'a [u8]> for Base<'a> {
    fn from(buf: &'a [u8]) -> Base<'a> {
        let mtype: MessageType = slice_to_u16(&buf[0..2]).into();
        let id = slice_to_u16(&buf[2..4]);
        let refers_to = slice_to_u16(&buf[4..6]);
        let sent_tv = TimeVal::from(&buf[6..14]);
        let received_tv = TimeVal::from(&buf[14..22]);
        let size = slice_to_u32(&buf[22..26]);
        let payload = &buf[Self::BASE_SIZE..Self::BASE_SIZE + size as usize];
        // short read
        assert_eq!(payload.len(), size as usize);
        Base {
            mtype,
            id,
            refers_to,
            sent_tv,
            received_tv,
            size,
            payload,
        }
    }
}

impl<'a> Base<'a> {
    const BASE_SIZE: usize = 26;
    pub fn decode(&self) -> ServerMessage {
        match self.mtype {
            MessageType::CodecHeader => ServerMessage::CodecHeader(CodecHeader::from(self.payload)),
            MessageType::ServerSettings => {
                ServerMessage::ServerSettings(ServerSettings::from(self.payload))
            }
            MessageType::WireChunk => ServerMessage::WireChunk(WireChunk::from(self.payload)),
            MessageType::Time => ServerMessage::Time(Time::from(self.payload)),
            _ => todo!("didnt get to {:?}", self.mtype),
        }
    }
    pub fn total_size(&self) -> usize {
        Self::BASE_SIZE + self.size as usize
    }
}
impl<'a> SerializeMessage for Base<'a> {
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
impl<'a> SerializeMessage for ClientHello<'a> {
    fn as_buf(&self) -> Vec<u8> {
        let p_str = serde_json::to_string(&self).unwrap();
        let payload = p_str.as_bytes();
        let mut payload_len_buf = u32::to_le_bytes(payload.len() as u32).to_vec();
        payload_len_buf.extend_from_slice(payload);
        let payload = payload_len_buf;
        Base {
            mtype: MessageType::Hello,
            id: 0,
            refers_to: 0,
            sent_tv: TimeVal { sec: 0, usec: 0 },
            received_tv: TimeVal { sec: 0, usec: 0 },
            size: payload.len() as u32,
            payload: &payload,
        }
        .as_buf()
    }
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct ServerSettings {
    bufferMs: u32,
    latency: u32,
    muted: bool,
    volume: u8,
}
#[derive(Debug)]
pub struct OpusMetadata {
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub channel_count: u16,
}

impl From<&[u8]> for OpusMetadata {
    fn from(buf: &[u8]) -> OpusMetadata {
        let marker = slice_to_u32(&buf[0..4]);
        let sample_rate = slice_to_u32(&buf[4..8]);
        let bit_depth = slice_to_u16(&buf[8..10]);
        let channel_count = slice_to_u16(&buf[10..12]);
        OpusMetadata {
            sample_rate,
            bit_depth,
            channel_count,
        }
    }
}
#[derive(Debug)]
pub enum CodecMetadata<'a> {
    Opaque(&'a [u8]),
    Opus(OpusMetadata),
}
#[derive(Debug)]
pub struct CodecHeader<'a> {
    pub codec: &'a str,
    pub metadata: CodecMetadata<'a>,
}

#[derive(Debug)]
pub struct WireChunk<'a> {
    pub timestamp: TimeVal,
    pub payload: &'a [u8],
}
#[derive(Debug)]
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

pub struct Server {
    mac: String,
    hostname: String,
}

impl Server {
    pub fn new(mac: String, hostname: String) -> Server {
        Server { mac, hostname }
    }
    pub fn hello(&self) -> Vec<u8> {
        ClientHello {
            Arch: "x86_64",
            ClientName: "CoolClient",
            HostName: &self.hostname,
            ID: &self.mac,
            Instance: 1,
            MAC: &self.mac,
            SnapStreamProtocolVersion: 2,
            Version: "0.17.1",
            OS: "an os",
        }
        .as_buf()
    }
}
