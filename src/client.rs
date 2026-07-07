#[cfg(feature = "std")]
use std::net::SocketAddrV4;
use std::net::TcpStream;

use crate::proto::{
    Base, ClientHello, CodecHeader, ServerMessage, ServerSettings, Time, TimeVal, WireChunk,
};
use circular_buffer::CircularBuffer;

pub struct Client {
    mac: String,
    hostname: String,
    conn: TcpStream,
    handler: ConnectedClient,
}

pub enum Message<'a> {
    Expired,
    Nothing,
    UpdateTiming(Time),
    WireChunk(WireChunk<'a>, TimeVal),
    ServerSettings(ServerSettings),
    CodecHeader(CodecHeader<'a>),
}

#[derive(Debug)]
pub struct ConnectedClient {
    state: HandlerState,
    time_base: TimeVal,
    last_time_sent: TimeVal,
    latency_buf: CircularBuffer<20, TimeVal>, // FIXME
    sorted_latency_buf: Vec<TimeVal>,
    hdr_buf: Vec<u8>,
    pkt_id: u16,
    server_buffer_ms: TimeVal,
    local_latency: TimeVal,
    last_sent_time: TimeVal,
    latency: TimeVal,
}

#[derive(Debug)]
enum HandlerState {
    UpdateTiming,
    ReadingHeader,
    ReadingPacket(u32),
}
#[derive(Debug)]
pub enum Event<'a> {
    UpdateTiming,
    HeaderReceived(&'a [u8]),
    PacketReceived(&'a [u8]),
}
#[derive(Debug)]
pub enum Action {
    ReadHeader,
    UpdateTiming,
    ReadPacket(u32),
}

impl ConnectedClient {
    fn new(time_base_us: u64) -> anyhow::Result<ConnectedClient> {
        let latency_buf = CircularBuffer::new();
        let cap = latency_buf.capacity();
        let tv_zero = TimeVal { sec: 0, usec: 0 };

        Ok(ConnectedClient {
            state: HandlerState::ReadingHeader,
            time_base: TimeVal::from(time_base_us),
            latency_buf,
            hdr_buf: vec![0; Base::BASE_SIZE],
            //pkt_buf: vec![0; 9000],
            sorted_latency_buf: vec![tv_zero; cap],
            pkt_id: 0,
            last_time_sent: TimeVal::from(time_base_us),
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
        })
    }

    pub fn synchronized(&self) -> bool {
        self.latency_buf.len() == self.sorted_latency_buf.len()
    }

    fn send_time(&mut self, time_us: u64, buf: &mut [u8]) {
        let tv = TimeVal::from(time_us);
        self.last_sent_time = tv;
        let t = Time::as_buf(self.pkt_id as u16, tv, tv, tv);
        self.pkt_id.wrapping_add(1);
        buf.copy_from_slice(t.as_slice());
    }

    /*
    fn fill_latency_buf(&mut self, time_us: u64) -> anyhow::Result<()> {
        let lts = TimeVal::from(time_us) - self.last_time_sent;

        // Want to fill the initial latency buffer fairly quickly (1ms between iters)
        // afterwards, a measurement a second should be OK
        if (!self.synchronized() && (lts.sec > 0 || lts.usec > 100_000)) || lts.sec >= 1 {
        }
        Ok(())
    }
    */

    pub fn tick(&mut self) -> Action {
        match &self.state {
            HandlerState::UpdateTiming => Action::UpdateTiming,
            HandlerState::ReadingHeader => Action::ReadHeader,
            HandlerState::ReadingPacket(size) => Action::ReadPacket(*size),
        }
    }

    // TODO get rid of anyhow
    pub fn handle_event<'m>(&mut self, event: Event<'m>, time_us: u64) -> Message<'m> {
        /*
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
        */
        match event {
            Event::UpdateTiming => {
                self.state = HandlerState::ReadingHeader;

                let tv = TimeVal::from(time_us);
                self.last_sent_time = tv;
                let t = Time::as_buf(self.pkt_id as u16, tv, tv, tv);
                self.pkt_id.wrapping_add(1);
                self.last_time_sent = TimeVal::from(time_us);
                Message::UpdateTiming(t)
            }
            Event::HeaderReceived(header_data) => {
                // TODO: unnecessary conversion, should store Base
                self.hdr_buf.copy_from_slice(&header_data);
                let base = Base::from(self.hdr_buf.as_slice());

                self.state = HandlerState::ReadingPacket(base.size);
                Message::Nothing
            }
            Event::PacketReceived(packet_data) => {
                let base = Base::from(self.hdr_buf.as_slice());

                // Reset state for next message; FIXME this should be after process_packet
                // but getting borrow issues
                let lts = TimeVal::from(time_us) - self.last_time_sent;
                if (!self.synchronized() && (lts.sec > 0 || lts.usec > 100_000)) || lts.sec >= 1 {
                    self.state = HandlerState::UpdateTiming;
                } else {
                    self.state = HandlerState::ReadingHeader;
                }
                self.process_packet(&base, &packet_data, time_us)
            }
        }
    }

    fn process_packet<'m>(
        &mut self,
        base: &Base,
        packet_data: &'m [u8],
        time_us: u64,
    ) -> Message<'m> {
        let decoded_m = base.decode(packet_data);
        let recv_ts = TimeVal::from(time_us) - self.time_base;
        match decoded_m {
            ServerMessage::Time(t) => {
                let c2s = base.received_tv /*(server) */ - self.last_sent_time /* client */ /*+ LAT (?) */;
                let s2c = recv_ts /*recv (systemtime::now()) */ - base.sent_tv /*(server) + LAT (?)*/;
                let lat = (c2s + s2c) / 2;

                // t.latency is actually "time-base conversion" from server-tbase to client-tbase
                self.latency_buf.push_back(lat + t.latency);

                for (i, tv) in self.latency_buf.iter().enumerate() {
                    self.sorted_latency_buf[i] = *tv;
                }

                self.sorted_latency_buf.sort();
                let slb_len = self.sorted_latency_buf.len();
                let nz_samples = &self.sorted_latency_buf[slb_len - self.latency_buf.len()..];
                self.latency = nz_samples[nz_samples.len() / 2];
                Message::Nothing
            }
            ServerMessage::WireChunk(wc) => {
                let t_c = wc.timestamp - self.latency;
                let audible_at = t_c + self.server_buffer_ms - self.local_latency;

                let cmp = audible_at - recv_ts;
                println!(
                    "wc ts {:?}, lat {:?}, recv_ts {:?}",
                    wc.timestamp, self.latency, recv_ts
                );
                if cmp.sec < 0 {
                    println!("Value in the past from net; dropping ({cmp:?})");
                    println!(
                        "now is {:?}, wirechunk ts is {:?}, latency is {:?}",
                        recv_ts, wc.timestamp, self.latency
                    );
                    println!("cicbuf {:?}", self.sorted_latency_buf);
                    return Message::Expired;
                }

                Message::WireChunk(wc, audible_at)
            }
            ServerMessage::ServerSettings(s) => {
                self.server_buffer_ms = TimeVal::from_millis(s.bufferMs as i32);
                self.local_latency = TimeVal::from_millis(s.latency as i32);
                Message::ServerSettings(s)
            }
            ServerMessage::CodecHeader(ch) => Message::CodecHeader(ch),
        }
    }

    /// Median latency out of the last measurements
    pub fn latency_to_server(&self) -> TimeVal {
        self.latency
    }

    pub fn time_base(&self) -> TimeVal {
        self.time_base
    }
}

#[cfg(feature = "std")]
use std::io::{Read, Write};
#[cfg(feature = "std")]
impl Client {
    pub fn new(
        mac: String,
        hostname: String,
        time_us: u64,
        dst: SocketAddrV4,
    ) -> anyhow::Result<Client> {
        let mut conn = TcpStream::connect(dst)?;
        let cc = ConnectedClient::new(time_us)?;

        let hello = ClientHello {
            Arch: std::env::consts::ARCH,
            ClientName: "CoolClient",
            HostName: &hostname,
            ID: &mac,
            Instance: 1,
            MAC: &mac,
            SnapStreamProtocolVersion: 2,
            Version: "0.17.1",
            OS: std::env::consts::OS,
        };

        match conn.set_nodelay(true) {
            Ok(()) => (),
            Err(e) => log::error!("Failed to set nodelay on connection: {:?}", e),
        }
        conn.set_read_timeout(Some(std::time::Duration::from_secs(1)))?;
        conn.write_all(&hello.as_buf())?;
        Ok(Client {
            mac,
            hostname,
            conn,
            handler: cc,
        })
    }
    pub fn tick<'m>(
        &mut self,
        time_us: u64,
        packet_buf: &'m mut [u8],
    ) -> anyhow::Result<Message<'m>> {
        loop {
            // TODO: here handle pending tick/time
            let action = self.handler.tick();

            match action {
                Action::ReadHeader => {
                    let mut header_buf = vec![0; Base::BASE_SIZE];

                    match self.conn.read_exact(&mut header_buf) {
                        Ok(()) => {
                            // let b = Base::from(header_buf.as_slice());
                            let event = Event::HeaderReceived(header_buf.as_slice());
                            self.handler.handle_event(event, time_us);
                            // Continue loop to get next action
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            return Ok(Message::Nothing);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }

                Action::ReadPacket(size) => {
                    match self.conn.read_exact(&mut packet_buf[0..size as usize]) {
                        Ok(()) => {
                            let event = Event::PacketReceived(packet_buf);
                            return Ok(self.handler.handle_event(event, time_us));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            return Ok(Message::Nothing);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }

                Action::UpdateTiming => {
                    self.handler.handle_event(Event::UpdateTiming, time_us);
                }
            }
        }
    }
}
