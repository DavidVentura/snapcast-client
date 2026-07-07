use crate::proto::{
    Base, ClientHello, CodecHeader, ServerMessage, ServerSettings, Time, TimeVal, WireChunk,
};
use anyhow::Context;
use circular_buffer::CircularBuffer;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub struct Client {
    mac: String,
    hostname: String,
}

pub enum Message<'a> {
    /// A WireChunk whose audible time already passed; carries how late it was
    Expired(TimeVal),
    Nothing,
    WireChunk(WireChunk<'a>, TimeVal),
    ServerSettings(ServerSettings),
    CodecHeader(CodecHeader<'a>),
}

#[derive(Debug)]
pub struct ConnectedClient {
    conn: TcpStream,
    time_base: Instant,
    last_time_sent: Instant,
    latency_buf: CircularBuffer<20, TimeVal>, // FIXME
    sorted_latency_buf: Vec<TimeVal>,
    hdr_buf: Vec<u8>,
    pkt_buf: Vec<u8>,
    pkt_id: u16,
    server_buffer_ms: TimeVal,
    local_latency: TimeVal,
    last_sent_time: TimeVal,
    latency: TimeVal,
}

impl ConnectedClient {
    fn new(conn: TcpStream) -> anyhow::Result<ConnectedClient> {
        match conn.set_nodelay(true) {
            Ok(()) => (),
            Err(e) => log::error!("Failed to set nodelay on connection: {:?}", e),
        }
        let time_base = Instant::now();
        let latency_buf = CircularBuffer::new();
        let cap = latency_buf.capacity();
        let tv_zero = TimeVal { sec: 0, usec: 0 };

        conn.set_read_timeout(Some(Duration::from_secs(1)))?;
        Ok(ConnectedClient {
            conn,
            time_base,
            latency_buf,
            hdr_buf: vec![0; Base::BASE_SIZE],
            pkt_buf: vec![0; 9000],
            sorted_latency_buf: vec![tv_zero; cap],
            pkt_id: 0,
            last_time_sent: Instant::now(),
            server_buffer_ms: TimeVal {
                sec: 0,
                usec: 999_999,
            },
            local_latency: TimeVal { sec: 0, usec: 0 },
            last_sent_time: TimeVal { sec: 0, usec: 0 },
            latency: TimeVal {
                sec: 0,
                usec: 1_000,
            },
            //data_buf: Box::new(CircularBuffer::new()),
        })
    }

    pub fn synchronized(&self) -> bool {
        self.latency_buf.len() == self.sorted_latency_buf.len()
    }
    fn send_hello(&mut self, h: ClientHello) -> anyhow::Result<()> {
        let b = h.as_buf();
        self.conn.write_all(&b)?;
        Ok(())
    }

    fn send_time(&mut self) -> anyhow::Result<()> {
        let now = self.time_base.elapsed();
        let tv = TimeVal {
            sec: now.as_secs() as i32, // allows for 68 years of uptime
            usec: now.subsec_micros() as i32,
        };
        self.last_sent_time = tv;
        let t = Time::as_buf(self.pkt_id as u16, tv, tv, tv);
        self.pkt_id += 1;
        self.conn.write_all(&t)?;
        Ok(())
    }

    fn fill_latency_buf(&mut self) -> anyhow::Result<()> {
        let lts = self.last_time_sent.elapsed();

        // Want to fill the initial latency buffer fairly quickly (1ms between iters)
        // afterwards, a measurement a second should be OK
        if (!self.synchronized() && lts.as_millis() > 0) || lts.as_secs() >= 1 {
            self.send_time()?;
            self.last_time_sent = Instant::now();
        }
        Ok(())
    }

    pub fn tick(&mut self) -> anyhow::Result<Message> {
        self.fill_latency_buf()?;

        let r = self.conn.read_exact(&mut self.hdr_buf);
        match r {
            Ok(()) => (),
            Err(e) => match e.kind() {
                std::io::ErrorKind::WouldBlock => {
                    return Ok(Message::Nothing);
                }
                _ => return Err(e.into()),
            },
        }
        let b = Base::from(self.hdr_buf.as_slice());
        if b.size as usize > self.pkt_buf.len() {
            log::warn!("Resizing pkt buf to {}", b.size);
            println!("Resizing pkt buf to {}", b.size);
            // pcm data is up to 4880b; flac is up to 9k~
            self.pkt_buf.resize(b.size as usize, 0);
        }
        self.conn
            .read_exact(&mut self.pkt_buf[0..b.size as usize])
            .context("cannot read pkt")?;

        let decoded_m = b.decode(&self.pkt_buf[0..b.size as usize]);
        let recv_ts = TimeVal::from(self.time_base.elapsed());
        match decoded_m {
            ServerMessage::Time(_) => {
                // c2s = clock_offset + uplink_delay; s2c = -clock_offset + downlink_delay
                // their difference cancels the (symmetric) network delay, leaving the
                // server-to-client clock offset; summing would cancel the offset instead
                let c2s = b.received_tv /*(server) */ - self.last_sent_time /* client */;
                let s2c = recv_ts /*recv (systemtime::now()) */ - b.sent_tv /*(server)*/;
                let diff = c2s - s2c;
                // TimeVal::div truncates sec and usec separately, which loses up to
                // 500ms when sec is odd; divide in microseconds instead
                let diff_us = (diff.sec as i64) * 1_000_000 + diff.usec as i64;
                let offset_us = diff_us / 2;
                let offset = TimeVal {
                    sec: (offset_us / 1_000_000) as i32,
                    usec: (offset_us % 1_000_000) as i32,
                }
                .normalize();

                self.latency_buf.push_back(offset);

                for (i, tv) in self.latency_buf.iter().enumerate() {
                    self.sorted_latency_buf[i] = *tv;
                }

                self.sorted_latency_buf.sort();
                let slb_len = self.sorted_latency_buf.len();
                let nz_samples = &self.sorted_latency_buf[slb_len - self.latency_buf.len()..];
                self.latency = nz_samples[nz_samples.len() / 2];
                Ok(Message::Nothing)
            }
            ServerMessage::WireChunk(wc) => {
                let t_c = wc.timestamp - self.latency;
                let tb = self.time_base.elapsed();
                let audible_at = t_c + self.server_buffer_ms - self.local_latency;

                let cmp = audible_at - tb.into();
                if cmp.sec < 0 {
                    // deliberately silent: printing here (blocking UART on embedded)
                    // makes drop-processing slower than the chunk rate, so a client
                    // that falls behind can never catch up; the caller rate-limits
                    return Ok(Message::Expired(cmp));
                }

                Ok(Message::WireChunk(wc, audible_at))
            }
            ServerMessage::ServerSettings(s) => {
                self.server_buffer_ms = TimeVal::from_millis(s.bufferMs as i32);
                self.local_latency = TimeVal::from_millis(s.latency as i32);
                Ok(Message::ServerSettings(s))
            }
            ServerMessage::CodecHeader(ch) => Ok(Message::CodecHeader(ch)),
        }
    }

    /// Median latency out of the last measurements
    pub fn latency_to_server(&self) -> TimeVal {
        self.latency
    }

    pub fn time_base(&self) -> Instant {
        self.time_base
    }
}

impl Client {
    pub fn connect<A: ToSocketAddrs>(&self, dst: A) -> anyhow::Result<ConnectedClient> {
        let conn = TcpStream::connect(dst)?;
        let mut cc = ConnectedClient::new(conn)?;

        let hello = ClientHello {
            Arch: std::env::consts::ARCH,
            ClientName: "CoolClient",
            HostName: &self.hostname,
            ID: &self.mac,
            Instance: 1,
            MAC: &self.mac,
            SnapStreamProtocolVersion: 2,
            Version: "0.17.1",
            OS: std::env::consts::OS,
        };
        cc.send_hello(hello)?;
        Ok(cc)
    }
    pub fn new(mac: String, hostname: String) -> Client {
        Client { mac, hostname }
    }
}
