use crate::proto::{Base, ClientHello, ServerMessage, Time, TimeVal};
use circular_buffer::CircularBuffer;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub struct Client {
    mac: String,
    hostname: String,
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
}

impl ConnectedClient {
    fn new(conn: TcpStream) -> anyhow::Result<ConnectedClient> {
        match conn.set_nodelay(true) {
            Ok(()) => (),
            Err(e) => println!("Failed to set nodelay on connection: {:?}", e),
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
            pkt_buf: vec![0; 6000], // pcm data is up to 4880b i think
            sorted_latency_buf: vec![tv_zero; cap],
            pkt_id: 0,
            last_time_sent: Instant::now(),
        })
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

    // TODO: should be a narrower type, filter time out?
    pub fn tick(&mut self) -> anyhow::Result<ServerMessage> {
        let lts = self.last_time_sent.elapsed();
        let filling_buf = self.latency_buf.len() < self.latency_buf.capacity();
        let empty = self.latency_buf.len() == 0;

        // Want to fill the initial latency buffer fairly quickly (100ms between iters)
        // afterwards, a measurement a second should be OK
        if empty || (filling_buf && lts.as_millis() > 0) || lts.as_secs() >= 1 {
            self.send_time()?;
            self.last_time_sent = Instant::now();
        }

        self.conn.read_exact(&mut self.hdr_buf)?;
        let b = Base::from(self.hdr_buf.as_slice());
        self.conn
            .read_exact(&mut self.pkt_buf[0..b.size as usize])?;

        let decoded_m = b.decode(&self.pkt_buf[0..b.size as usize]);
        match decoded_m {
            ServerMessage::Time(t) => {
                let tbase_adj = t.latency - self.time_zero.into();
                self.latency_buf.push_back(tbase_adj);
                Ok(ServerMessage::Nothing)
            }
            any => Ok(any),
        }
    }

    /// Median latency out of the last measurements
    pub fn latency_to_server(&mut self) -> TimeVal {
        if self.latency_buf.len() == 0 {
            return TimeVal {
                sec: 0,
                usec: 1_000,
            };
        }

        for (i, tv) in self.latency_buf.iter().enumerate() {
            self.sorted_latency_buf[i] = *tv;
        }

        let slb_len = self.sorted_latency_buf.len();
        self.sorted_latency_buf.sort();
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
            OS: "an os",
        };
        cc.send_hello(hello)?;
        Ok(cc)
    }
    pub fn new(mac: String, hostname: String) -> Client {
        Client { mac, hostname }
    }
}
