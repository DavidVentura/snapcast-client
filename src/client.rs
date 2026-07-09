use crate::proto::{
    Base, ClientHello, CodecHeader, ServerMessage, ServerSettings, Time, TimeVal, WireChunk,
};
pub use crate::framing::{Action, Event};
use crate::framing::Framing;
use circular_buffer::CircularBuffer;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub enum Message<'a> {
    /// A WireChunk whose audible time already passed; carries how late it was
    Expired(TimeVal),
    Nothing,
    WireChunk(WireChunk<'a>, TimeVal),
    ServerSettings(ServerSettings),
    CodecHeader(CodecHeader<'a>),
}

const LATENCY_SAMPLES: usize = 20;

/// Socket-free snapclient protocol core. It never reads or writes bytes; the
/// caller drives it by feeding `Event`s (satisfying the requested `next_action`)
/// and draining `poll_transmit`, injecting a monotonic `now_us` for every step.
pub struct ClientMachine {
    framing: Framing,
    latency_buf: CircularBuffer<LATENCY_SAMPLES, TimeVal>,
    sorted_latency_buf: Vec<TimeVal>,
    pkt_id: u16,
    server_buffer_ms: TimeVal,
    local_latency: TimeVal,
    /// Client timestamp stamped into the last Time request; read by the offset math.
    last_sent_time: TimeVal,
    /// When the last Time request was emitted; gates the request cadence.
    last_time_sent_us: i64,
    /// Median server-to-client clock offset.
    clock_offset: TimeVal,
}

impl Default for ClientMachine {
    fn default() -> ClientMachine {
        ClientMachine::new()
    }
}

impl ClientMachine {
    pub fn new() -> ClientMachine {
        let tv_zero = TimeVal { sec: 0, usec: 0 };
        ClientMachine {
            framing: Framing::new(),
            latency_buf: CircularBuffer::new(),
            sorted_latency_buf: vec![tv_zero; LATENCY_SAMPLES],
            pkt_id: 0,
            server_buffer_ms: TimeVal {
                sec: 0,
                usec: 999_999,
            },
            local_latency: tv_zero,
            last_sent_time: tv_zero,
            last_time_sent_us: 0,
            clock_offset: TimeVal {
                sec: 0,
                usec: 1_000,
            },
        }
    }

    pub fn synchronized(&self) -> bool {
        self.latency_buf.len() == self.sorted_latency_buf.len()
    }

    /// Median server-to-client clock offset out of the last measurements.
    pub fn clock_offset(&self) -> TimeVal {
        self.clock_offset
    }

    pub fn next_action(&self) -> Action {
        self.framing.next_action()
    }

    /// Emit a timer-driven Time request into `out` when one is due, returning its
    /// length. Dense sampling (>=1ms apart) until the offset buffer fills, once a
    /// second afterwards. `out` must hold at least [`Time::WIRE_SIZE`] bytes.
    pub fn poll_transmit(&mut self, now_us: i64, out: &mut [u8]) -> Option<usize> {
        let elapsed = now_us - self.last_time_sent_us;
        let due = (!self.synchronized() && elapsed >= 1_000) || elapsed >= 1_000_000;
        if !due {
            return None;
        }
        let tv = TimeVal::from_micros(now_us);
        self.last_sent_time = tv;
        self.last_time_sent_us = now_us;
        let n = Time::write(out, self.pkt_id, 0, tv, tv, tv);
        self.pkt_id = self.pkt_id.wrapping_add(1);
        Some(n)
    }

    pub fn handle_event<'a>(&mut self, event: Event<'a>, now_us: i64) -> Message<'a> {
        match event {
            Event::HeaderReceived(bytes) => {
                self.framing.on_header(bytes);
                Message::Nothing
            }
            Event::PacketReceived(bytes) => {
                let base = self.framing.take_base();
                self.process_packet(base, bytes, now_us)
            }
        }
    }

    fn process_packet<'a>(&mut self, base: Base, payload: &'a [u8], now_us: i64) -> Message<'a> {
        let recv_ts = TimeVal::from_micros(now_us);
        match base.decode(payload) {
            ServerMessage::Time(_) => {
                // c2s = clock_offset + uplink_delay; s2c = -clock_offset + downlink_delay
                // their difference cancels the (symmetric) network delay, leaving the
                // server-to-client clock offset; summing would cancel the offset instead
                let c2s = base.received_tv - self.last_sent_time;
                let s2c = recv_ts - base.sent_tv;
                let diff = c2s - s2c;
                // TimeVal::div truncates sec and usec separately, which loses up to
                // 500ms when sec is odd; divide in microseconds instead
                let offset = TimeVal::from_micros(diff.to_micros() / 2).normalize();

                self.latency_buf.push_back(offset);
                for (i, tv) in self.latency_buf.iter().enumerate() {
                    self.sorted_latency_buf[i] = *tv;
                }
                self.sorted_latency_buf.sort();
                let slb_len = self.sorted_latency_buf.len();
                let nz_samples = &self.sorted_latency_buf[slb_len - self.latency_buf.len()..];
                self.clock_offset = nz_samples[nz_samples.len() / 2];
                Message::Nothing
            }
            ServerMessage::WireChunk(wc) => {
                let t_c = wc.timestamp - self.clock_offset;
                let audible_at = t_c + self.server_buffer_ms - self.local_latency;

                let cmp = audible_at - recv_ts;
                if cmp.sec < 0 {
                    // deliberately silent: printing here (blocking UART on embedded)
                    // makes drop-processing slower than the chunk rate, so a client
                    // that falls behind can never catch up; the caller rate-limits
                    return Message::Expired(cmp);
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
}

pub struct Client {
    mac: String,
    hostname: String,
}

/// Blocking imperative shell around [`ClientMachine`]: owns the TcpStream and the
/// server time base, drives one read step per `tick`, and injects wall-clock time.
pub struct ConnectedClient {
    conn: TcpStream,
    time_base: Instant,
    machine: ClientMachine,
    hdr_buf: Vec<u8>,
    pkt_buf: Vec<u8>,
    tx_buf: [u8; Time::WIRE_SIZE],
}

impl ConnectedClient {
    fn new(conn: TcpStream) -> anyhow::Result<ConnectedClient> {
        match conn.set_nodelay(true) {
            Ok(()) => (),
            Err(e) => log::error!("Failed to set nodelay on connection: {:?}", e),
        }
        conn.set_read_timeout(Some(Duration::from_secs(1)))?;
        Ok(ConnectedClient {
            conn,
            time_base: Instant::now(),
            machine: ClientMachine::new(),
            hdr_buf: vec![0; Base::BASE_SIZE],
            pkt_buf: vec![0; 9000],
            tx_buf: [0; Time::WIRE_SIZE],
        })
    }

    fn now_us(&self) -> i64 {
        self.time_base.elapsed().as_micros() as i64
    }

    pub fn synchronized(&self) -> bool {
        self.machine.synchronized()
    }

    /// Median server-to-client clock offset out of the last measurements.
    pub fn clock_offset(&self) -> TimeVal {
        self.machine.clock_offset()
    }

    pub fn time_base(&self) -> Instant {
        self.time_base
    }

    pub fn tick(&mut self) -> anyhow::Result<Message<'_>> {
        let tx_now = self.now_us();
        while let Some(n) = self.machine.poll_transmit(tx_now, &mut self.tx_buf) {
            self.conn.write_all(&self.tx_buf[..n])?;
        }

        loop {
            match self.machine.next_action() {
                Action::ReadHeader => match self.conn.read_exact(&mut self.hdr_buf) {
                    Ok(()) => {
                        let ev = Event::HeaderReceived(&self.hdr_buf);
                        // returns Nothing and transitions to ReadPacket; loop to read it
                        let _ = self.machine.handle_event(ev, tx_now);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        return Ok(Message::Nothing);
                    }
                    Err(e) => return Err(e.into()),
                },
                Action::ReadPacket(size) => {
                    let size = size as usize;
                    if size > self.pkt_buf.len() {
                        log::warn!("Resizing pkt buf to {}", size);
                        // pcm data is up to 4880b; flac is up to 9k~
                        self.pkt_buf.resize(size, 0);
                    }
                    match self.conn.read_exact(&mut self.pkt_buf[0..size]) {
                        Ok(()) => {
                            let rx_now = self.now_us();
                            let ev = Event::PacketReceived(&self.pkt_buf[0..size]);
                            return Ok(self.machine.handle_event(ev, rx_now));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            return Ok(Message::Nothing);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
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
        cc.conn.write_all(&hello.as_buf())?;
        Ok(cc)
    }
    pub fn new(mac: String, hostname: String) -> Client {
        Client { mac, hostname }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::ClientMessage;

    #[test]
    fn poll_transmit_cadence_and_frame() {
        let mut m = ClientMachine::new();
        let mut buf = [0u8; 64];

        // at t=0, no time has elapsed since construction: nothing due yet
        assert!(m.poll_transmit(0, &mut buf).is_none());

        // 1ms in, unsynchronized: a request is due
        let n = m.poll_transmit(1_000, &mut buf).unwrap();
        assert_eq!(n, Time::WIRE_SIZE);
        let base = Base::from(&buf[0..Base::BASE_SIZE]);
        assert_eq!(base.id, 0);
        match base.decode_c(&buf[Base::BASE_SIZE..n]) {
            ClientMessage::Time(_) => {}
            other => panic!("expected Time, got {other:?}"),
        }

        // immediately after, nothing is due
        assert!(m.poll_transmit(1_000, &mut buf).is_none());
        // still too soon 0.5ms later
        assert!(m.poll_transmit(1_500, &mut buf).is_none());

        // 1ms after the last one, the next request increments pkt_id
        let n = m.poll_transmit(2_000, &mut buf).unwrap();
        let base = Base::from(&buf[0..Base::BASE_SIZE]);
        assert_eq!(base.id, 1);
        assert_eq!(n, Time::WIRE_SIZE);
    }

    // Drives a Time round-trip with a known clock offset and symmetric one-way
    // delay: the client sends at client-time `s_us`, the server stamps both its
    // received/sent at `s_us + offset + delay`, and the reply lands at
    // `s_us + 2*delay`. The offset math must recover `offset` regardless of delay.
    fn feed_time_reply(m: &mut ClientMachine, s_us: i64, offset_us: i64, delay_us: i64) {
        let mut buf = [0u8; 64];
        m.poll_transmit(s_us, &mut buf)
            .expect("time request should be due");
        let server_t = TimeVal::from_micros(s_us + offset_us + delay_us);
        let reply = Time::as_buf(0, 0, server_t, server_t, TimeVal { sec: 0, usec: 0 });
        let (hdr, payload) = reply.split_at(Base::BASE_SIZE);
        m.handle_event(Event::HeaderReceived(hdr), 0);
        m.handle_event(Event::PacketReceived(payload), s_us + 2 * delay_us);
    }

    #[test]
    fn offset_converges_to_known_value() {
        let mut m = ClientMachine::new();
        let offset = 50_000;
        for k in 0..LATENCY_SAMPLES as i64 {
            feed_time_reply(&mut m, (k + 1) * 2_000, offset, 10_000);
        }
        assert!(m.synchronized());
        assert_eq!(m.clock_offset(), TimeVal::from_micros(offset));
    }

    fn feed_wire_chunk(m: &mut ClientMachine, ts: TimeVal, now_us: i64) -> Message<'static> {
        // as_buf owns its bytes; leak them so the returned Message can borrow 'static
        let data: &'static [u8] = Box::leak(vec![0u8; 8].into_boxed_slice());
        let wc = WireChunk {
            timestamp: ts,
            payload: data,
        };
        let buf: &'static [u8] =
            Box::leak(wc.as_buf(0, TimeVal { sec: 0, usec: 0 }).into_boxed_slice());
        let (hdr, payload) = buf.split_at(Base::BASE_SIZE);
        m.handle_event(Event::HeaderReceived(hdr), 0);
        m.handle_event(Event::PacketReceived(payload), now_us)
    }

    #[test]
    fn wire_chunk_future_is_audible_past_is_expired() {
        let mut m = ClientMachine::new();
        // 1s server buffer, no local latency
        let ss = ServerSettings {
            bufferMs: 1000,
            latency: 0,
            muted: false,
            volume: 100,
        };
        let buf = ss.as_buf(0, TimeVal { sec: 0, usec: 0 });
        let (hdr, payload) = buf.split_at(Base::BASE_SIZE);
        m.handle_event(Event::HeaderReceived(hdr), 0);
        m.handle_event(Event::PacketReceived(payload), 0);

        // a chunk timestamped 1s out, received now, is audible in the future
        match feed_wire_chunk(&mut m, TimeVal::from_micros(1_000_000), 0) {
            Message::WireChunk(_, audible_at) => assert!(audible_at.sec >= 1),
            _ => panic!("expected WireChunk"),
        }

        // a chunk timestamped at 0, received 5s later, is long expired
        match feed_wire_chunk(&mut m, TimeVal::from_micros(0), 5_000_000) {
            Message::Expired(lateness) => assert!(lateness.sec < 0),
            _ => panic!("expected Expired"),
        }
    }
}
