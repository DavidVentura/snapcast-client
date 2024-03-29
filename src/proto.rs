use std::ops::{Add, Sub};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub struct TimeVal {
    // order of fields matter for Ord/PartialOrd derives
    pub sec: i32,
    pub usec: i32,
}

impl TimeVal {
    fn normalize(mut self) -> Self {
        while self.usec > 1_000_000 {
            self.usec -= 1_000_000;
            self.sec += 1;
        }
        while self.usec < 0 {
            self.usec += 1_000_000;
            self.sec -= 1;
        }

        self
    }
    pub fn from_millis(millis: i32) -> TimeVal {
        TimeVal {
            sec: 0,
            usec: millis * 1000,
        }
        .normalize()
    }
    pub fn millis(&self) -> anyhow::Result<u16> {
        let s = self.normalize();
        if s.sec != 0 {
            anyhow::bail!(format!("sec {s:?} is != 0"));
        }
        if s.usec < 0 {
            anyhow::bail!(format!("usec {s:?} is < 0"));
        }
        Ok((s.usec / 1000) as u16)
    }
}

impl From<&[u8]> for TimeVal {
    fn from(buf: &[u8]) -> TimeVal {
        TimeVal {
            sec: slice_to_i32(&buf[0..4]),
            usec: slice_to_i32(&buf[4..8]),
        }
    }
}

impl From<Duration> for TimeVal {
    fn from(d: Duration) -> TimeVal {
        TimeVal {
            sec: d.as_secs() as i32,
            usec: d.subsec_micros() as i32,
        }
    }
}

impl Add<TimeVal> for TimeVal {
    type Output = TimeVal;
    fn add(self, other: TimeVal) -> TimeVal {
        let sec = self.sec + other.sec;
        let usec = self.usec + other.usec;
        TimeVal { sec, usec }.normalize()
    }
}
impl Sub<TimeVal> for TimeVal {
    type Output = TimeVal;
    fn sub(self, other: TimeVal) -> TimeVal {
        let sec = self.sec - other.sec;
        let usec = self.usec - other.usec;
        TimeVal { sec, usec }.normalize()
    }
}

impl From<TimeVal> for Vec<u8> {
    fn from(tv: TimeVal) -> Vec<u8> {
        let mut v = Vec::with_capacity(8);
        v.extend_from_slice(&i32::to_le_bytes(tv.sec));
        v.extend_from_slice(&i32::to_le_bytes(tv.usec));
        v
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
    Nothing,
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
pub struct Base {
    mtype: MessageType,
    id: u16,
    refers_to: u16,
    pub(crate) sent_tv: TimeVal,
    pub(crate) received_tv: TimeVal,
    pub(crate) size: u32,
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
            "pcm" => CodecMetadata::Pcm(PcmMetadata::from(payload)),
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

impl<'a> From<&'a [u8]> for Base {
    fn from(buf: &'a [u8]) -> Base {
        let mtype: MessageType = slice_to_u16(&buf[0..2]).into();
        let id = slice_to_u16(&buf[2..4]);
        let refers_to = slice_to_u16(&buf[4..6]);
        let sent_tv = TimeVal::from(&buf[6..14]);
        let received_tv = TimeVal::from(&buf[14..22]);
        let size = slice_to_u32(&buf[22..26]);
        Base {
            mtype,
            id,
            refers_to,
            sent_tv,
            received_tv,
            size,
        }
    }
}

impl Base {
    pub const BASE_SIZE: usize = 26;

    pub fn decode<'a>(&self, payload: &'a [u8]) -> ServerMessage<'a> {
        match self.mtype {
            MessageType::CodecHeader => ServerMessage::CodecHeader(CodecHeader::from(payload)),
            MessageType::ServerSettings => {
                ServerMessage::ServerSettings(ServerSettings::from(payload))
            }
            MessageType::WireChunk => ServerMessage::WireChunk(WireChunk::from(payload)),
            MessageType::Time => ServerMessage::Time(Time::from(payload)),
            _ => todo!("didnt get to {:?}", self.mtype),
        }
    }

    fn as_buf(&self, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(payload.len() + Base::BASE_SIZE);

        buf.extend(u16::to_le_bytes(self.mtype as u16));
        buf.extend(u16::to_le_bytes(self.id));
        buf.extend(u16::to_le_bytes(self.refers_to));
        buf.extend(i32::to_le_bytes(self.sent_tv.sec));
        buf.extend(i32::to_le_bytes(self.sent_tv.usec));
        buf.extend(i32::to_le_bytes(self.received_tv.sec));
        buf.extend(i32::to_le_bytes(self.received_tv.usec));
        buf.extend(u32::to_le_bytes(self.size));
        buf.extend(payload);
        buf
    }
}

impl Time {
    // TODO: this should be a TimeReq which is mut
    // to prevent these stupid 8 byte allocations (latency)
    pub(crate) fn as_buf(
        id: u16,
        sent_tv: TimeVal,
        received_tv: TimeVal,
        latency: TimeVal,
    ) -> Vec<u8> {
        let payload = Vec::<u8>::from(latency);
        Base {
            mtype: MessageType::Time,
            id,
            refers_to: 0,
            sent_tv,
            received_tv,
            size: payload.len() as u32,
        }
        .as_buf(&payload)
    }
}
impl<'a> ClientHello<'a> {
    pub fn as_buf(&self) -> Vec<u8> {
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
        }
        .as_buf(&payload)
    }
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub(crate) struct ServerSettings {
    pub(crate) bufferMs: u32,
    pub(crate) latency: u32,
    pub(crate) muted: bool,
    pub(crate) volume: u8,
}
#[derive(Debug, Copy, Clone)]
pub struct OpusMetadata {
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub channel_count: u16,
}

impl From<&[u8]> for OpusMetadata {
    fn from(buf: &[u8]) -> OpusMetadata {
        let _marker = slice_to_u32(&buf[0..4]);
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
pub(crate) struct PcmMetadata<'a> {
    raw_payload: &'a [u8],
    pub(crate) channel_count: u16,
    pub(crate) audio_rate: u32,
    pub(crate) bit_depth: u16,
}

impl<'a> From<&'a [u8]> for PcmMetadata<'a> {
    fn from(buf: &'a [u8]) -> PcmMetadata<'a> {
        assert_eq!(buf[0..4], [b'R', b'I', b'F', b'F']);
        // +16 = remaining header len
        let format_tag = slice_to_u16(&buf[20..22]);
        assert_eq!(format_tag, 1); // PCM
        let channel_count = slice_to_u16(&buf[22..24]);
        let audio_rate = slice_to_u32(&buf[24..28]);
        let bit_depth = slice_to_u16(&buf[34..36]);
        PcmMetadata {
            raw_payload: buf,
            channel_count,
            bit_depth,
            audio_rate,
        }
    }
}

#[derive(Debug)]
pub enum CodecMetadata<'a> {
    Opaque(&'a [u8]),
    Pcm(PcmMetadata<'a>),
    Opus(OpusMetadata),
}

impl<'a> CodecMetadata<'a> {
    pub fn rate(&self) -> usize {
        match self {
            CodecMetadata::Opus(o) => o.sample_rate as usize,
            CodecMetadata::Pcm(p) => p.audio_rate as usize,
            _ => todo!(),
        }
    }
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
pub struct Time {
    pub(crate) latency: TimeVal,
}
#[allow(non_snake_case)]
#[derive(Serialize)]
pub struct ClientHello<'a> {
    pub MAC: &'a str,
    pub HostName: &'a str,
    pub Version: &'a str,
    pub ClientName: &'a str,
    pub OS: &'a str,
    pub Arch: &'a str,
    pub Instance: u8,
    pub ID: &'a str,
    pub SnapStreamProtocolVersion: u8, // this one shouldn't be pub
}
