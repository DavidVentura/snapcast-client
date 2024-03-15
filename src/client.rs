use crate::proto::{Base, ClientHello, ServerMessage, Time, TimeVal};
use circular_buffer::CircularBuffer;
use std::io::prelude::*;
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub struct Client {
    mac: String,
    hostname: String,
}

#[derive(Debug)]
pub struct ConnectedClient {
    conn_rx: TcpStream,
    conn_tx: TcpStream,
    time_zero: Duration,
    time_base: Instant,
    latency_buf: CircularBuffer<50, TimeVal>,
    sorted_latency_buf: Vec<TimeVal>,
    hdr_buf: Vec<u8>,
    pkt_buf: Vec<u8>,
    pkt_id: u16,
}

impl ConnectedClient {
    fn new(conn: TcpStream) -> anyhow::Result<ConnectedClient> {
        conn.set_nodelay(true)?;
        let time_base = Instant::now();
        let time_zero = time_base.elapsed();

        let conn_tx = conn.try_clone()?;
        let latency_buf = CircularBuffer::new();
        let cap = latency_buf.capacity();
        let tv_zero = TimeVal { sec: 0, usec: 0 };

        Ok(ConnectedClient {
            conn_rx: conn,
            conn_tx,
            time_base,
            time_zero,
            latency_buf,
            hdr_buf: vec![0; Base::BASE_SIZE],
            pkt_buf: vec![0; 6000], // pcm data is up to 4880b i think
            sorted_latency_buf: vec![tv_zero; cap],
            pkt_id: 0,
        })
    }

    fn send_hello(&mut self, h: ClientHello) -> anyhow::Result<()> {
        let b = h.as_buf();
        self.conn_tx.write_all(&b)?;
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
        self.conn_tx.write_all(&t)?;
        Ok(())
    }

    // TODO: should be a narrower type, filter time out?
    pub fn tick(&mut self) -> anyhow::Result<ServerMessage> {
        if self.latency_buf.len() < self.latency_buf.capacity() {
            self.send_time()?;
        }

        self.conn_rx.read_exact(&mut self.hdr_buf)?;
        let b = Base::from(self.hdr_buf.as_slice());
        self.conn_rx
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

    pub fn latency_to_server(&mut self) -> TimeVal {
        // FIXME alloc
        if self.latency_buf.len() == 0 {
            return TimeVal { sec: 0, usec: 0 };
        }
        for (i, tv) in self.latency_buf.iter().enumerate() {
            self.sorted_latency_buf[i] = *tv;
        }
        self.sorted_latency_buf.sort();
        self.sorted_latency_buf[self.sorted_latency_buf.len() / 2]
    }

    pub fn time_base(&self) -> Instant {
        self.time_base
    }
}

impl Client {
    pub fn connect(&self) -> anyhow::Result<ConnectedClient> {
        let conn = TcpStream::connect("192.168.2.131:1704")?;
        let mut cc = ConnectedClient::new(conn)?;

        let hello = ClientHello {
            Arch: "x86_64",
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
