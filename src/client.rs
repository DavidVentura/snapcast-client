use crate::proto::{Base, ClientHello, CodecHeader, ServerMessage, Time, TimeVal, WireChunk};
use circular_buffer::CircularBuffer;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub struct Client {
    mac: String,
    hostname: String,
}

pub enum Message<'a> {
    Nothing,
    WireChunk(WireChunk<'a>, TimeVal),
    PlaybackVolume(u8),
    CodecHeader(CodecHeader<'a>),
}

#[derive(Debug)]
pub struct ConnectedClient {
    conn: TcpStream,
    time_zero: Duration,
    time_base: Instant,
    last_time_sent: Instant,
    latency_buf: CircularBuffer<50, TimeVal>,
    sorted_latency_buf: Vec<TimeVal>,
    hdr_buf: Vec<u8>,
    pkt_buf: Vec<u8>,
    pkt_id: u16,
    server_buffer_ms: TimeVal,
    local_latency: TimeVal,
}

impl ConnectedClient {
    fn new(conn: TcpStream) -> anyhow::Result<ConnectedClient> {
        match conn.set_nodelay(true) {
            Ok(()) => (),
            Err(e) => log::error!("Failed to set nodelay on connection: {:?}", e),
        }
        let time_base = Instant::now();
        let time_zero = time_base.elapsed();

        let latency_buf = CircularBuffer::new();
        let cap = latency_buf.capacity();
        let tv_zero = TimeVal { sec: 0, usec: 0 };

        Ok(ConnectedClient {
            conn,
            time_base,
            time_zero,
            latency_buf,
            hdr_buf: vec![0; Base::BASE_SIZE],
            pkt_buf: vec![0; 9000], // pcm data is up to 4880b; localhost mtu gets up to 9k
            sorted_latency_buf: vec![tv_zero; cap],
            pkt_id: 0,
            last_time_sent: Instant::now(),
            server_buffer_ms: TimeVal {
                sec: 0,
                usec: 999_999,
            },
            local_latency: TimeVal { sec: 0, usec: 0 },
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
        let t = Time::as_buf(self.pkt_id as u16, tv, tv, tv);
        self.pkt_id += 1;
        self.conn.write_all(&t)?;
        Ok(())
    }

    fn fill_latency_buf(&mut self) -> anyhow::Result<()> {
        let lts = self.last_time_sent.elapsed();
        let filling_buf = self.latency_buf.len() < self.latency_buf.capacity();
        let empty = self.latency_buf.len() == 0;

        // Want to fill the initial latency buffer fairly quickly (1ms between iters)
        // afterwards, a measurement a second should be OK
        if empty || (filling_buf && lts.as_millis() > 0) || lts.as_secs() >= 1 {
            self.send_time()?;
            self.last_time_sent = Instant::now();
        }
        Ok(())
    }

    pub fn tick(&mut self) -> anyhow::Result<Message> {
        self.fill_latency_buf()?;

        self.conn.read_exact(&mut self.hdr_buf)?;
        let b = Base::from(self.hdr_buf.as_slice());
        self.conn
            .read_exact(&mut self.pkt_buf[0..b.size as usize])?;

        let decoded_m = b.decode(&self.pkt_buf[0..b.size as usize]);
        match decoded_m {
            ServerMessage::Time(t) => {
                self.latency_buf.push_back(t.latency);
                // t.latency may go down
                for (i, tv) in self.latency_buf.iter().enumerate() {
                    self.sorted_latency_buf[i] = *tv;
                }

                self.sorted_latency_buf.sort();
                Ok(Message::Nothing)
            }
            ServerMessage::WireChunk(wc) => {
                let t_s = wc.timestamp;
                let t_c = t_s - median_tbase;
                let audible_at = t_c + self.server_buffer_ms - self.local_latency;
                Ok(Message::WireChunk(wc, audible_at))
            }
            ServerMessage::ServerSettings(ref s) => {
                self.server_buffer_ms = TimeVal::from_millis(s.bufferMs as i32);
                self.local_latency = TimeVal::from_millis(s.latency as i32);
                Ok(Message::PlaybackVolume(s.volume))
            }
            ServerMessage::CodecHeader(ch) => Ok(Message::CodecHeader(ch)),
        }
    }

    /// Median latency out of the last measurements
    pub fn latency_to_server(&self) -> TimeVal {
        if self.latency_buf.len() == 0 {
            return TimeVal {
                sec: 0,
                usec: 1_000,
            };
        }

        let slb_len = self.sorted_latency_buf.len();
        let nz_samples = &self.sorted_latency_buf[slb_len - self.latency_buf.len()..];
        nz_samples[nz_samples.len() / 2]
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
