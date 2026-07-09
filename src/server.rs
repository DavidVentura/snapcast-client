use crate::framing::Framing;
pub use crate::framing::{Action, Event};
use crate::proto::{ClientHello, ClientMessage, TimeVal};

/// A semantic event decoded from a connected snapclient. The runtime turns these
/// into wire replies; the session deliberately does not, because a Time reply's
/// `sent_tv` must be stamped at the socket write, not here.
pub enum SessionOutput<'a> {
    None,
    Hello(ClientHello<'a>),
    /// A time-sync request. The reply must set `refers_to = id`,
    /// `received_tv = received`, and stamp `sent_tv` just before writing.
    TimeRequest {
        id: u16,
        client_sent: TimeVal,
        received: TimeVal,
    },
}

/// Socket-free server-side protocol core for one connected client. Mirrors
/// [`crate::client::ClientMachine`]: the runtime feeds `Event`s and injects the
/// server clock as `now`.
pub struct ServerSession {
    framing: Framing,
    greeted: bool,
}

impl Default for ServerSession {
    fn default() -> ServerSession {
        ServerSession::new()
    }
}

impl ServerSession {
    pub fn new() -> ServerSession {
        ServerSession {
            framing: Framing::new(),
            greeted: false,
        }
    }

    pub fn next_action(&self) -> Action {
        self.framing.next_action()
    }

    pub fn handle_event<'a>(
        &mut self,
        event: Event<'a>,
        now: TimeVal,
    ) -> anyhow::Result<SessionOutput<'a>> {
        match event {
            Event::HeaderReceived(bytes) => {
                self.framing.on_header(bytes)?;
                Ok(SessionOutput::None)
            }
            Event::PacketReceived(bytes) => {
                let base = self.framing.take_base();
                match base.decode_c(bytes)? {
                    ClientMessage::Hello(h) => {
                        self.greeted = true;
                        Ok(SessionOutput::Hello(h))
                    }
                    ClientMessage::Time(_) => {
                        if !self.greeted {
                            anyhow::bail!("received a Time request before Hello");
                        }
                        Ok(SessionOutput::TimeRequest {
                            id: base.id,
                            client_sent: base.sent_tv,
                            received: now,
                        })
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientMachine;
    use crate::proto::{Base, ClientHello, Time};

    fn hello_bytes() -> Vec<u8> {
        ClientHello {
            MAC: "aa:bb:cc:dd:ee:ff",
            HostName: "test",
            Version: "0.17.1",
            ClientName: "test",
            OS: "linux",
            Arch: "x86_64",
            Instance: 1,
            ID: "aa:bb:cc:dd:ee:ff",
            SnapStreamProtocolVersion: 2,
        }
        .as_buf()
    }

    fn feed<'a>(s: &mut ServerSession, buf: &'a [u8], now: TimeVal) -> SessionOutput<'a> {
        let (hdr, payload) = buf.split_at(Base::BASE_SIZE);
        s.handle_event(Event::HeaderReceived(hdr), now).unwrap();
        s.handle_event(Event::PacketReceived(payload), now).unwrap()
    }

    #[test]
    fn hello_then_time_request() {
        let mut s = ServerSession::new();
        let hello = hello_bytes();
        match feed(&mut s, &hello, TimeVal { sec: 0, usec: 0 }) {
            SessionOutput::Hello(_) => {}
            _ => panic!("expected Hello"),
        }

        let arrival = TimeVal {
            sec: 7,
            usec: 123,
        };
        let req = Time::as_buf(
            9,
            0,
            TimeVal { sec: 1, usec: 2 },
            TimeVal { sec: 0, usec: 0 },
            TimeVal { sec: 0, usec: 0 },
        );
        match feed(&mut s, &req, arrival) {
            SessionOutput::TimeRequest {
                id,
                client_sent,
                received,
            } => {
                assert_eq!(id, 9);
                assert_eq!(client_sent, TimeVal { sec: 1, usec: 2 });
                assert_eq!(received, arrival);
            }
            _ => panic!("expected TimeRequest"),
        }
    }

    #[test]
    fn time_before_hello_is_an_error() {
        let mut s = ServerSession::new();
        let req = Time::as_buf(
            0,
            0,
            TimeVal { sec: 1, usec: 2 },
            TimeVal { sec: 0, usec: 0 },
            TimeVal { sec: 0, usec: 0 },
        );
        let (hdr, payload) = req.split_at(Base::BASE_SIZE);
        s.handle_event(Event::HeaderReceived(hdr), TimeVal { sec: 0, usec: 0 })
            .unwrap();
        assert!(s
            .handle_event(Event::PacketReceived(payload), TimeVal { sec: 0, usec: 0 })
            .is_err());
    }

    // The strongest guard on the timestamp contract: no sockets, two machines.
    // The client's Time requests flow through a ServerSession, get serialized as
    // replies with server-clock timestamps, and flow back into the client, which
    // must recover the injected clock offset.
    #[test]
    fn client_recovers_offset_through_server_session() {
        let mut client = ClientMachine::new();
        let mut server = ServerSession::new();

        // greet the server so it will answer Time requests
        let hello = hello_bytes();
        {
            let (hdr, payload) = hello.split_at(Base::BASE_SIZE);
            server
                .handle_event(Event::HeaderReceived(hdr), TimeVal { sec: 0, usec: 0 })
                .unwrap();
            server
                .handle_event(Event::PacketReceived(payload), TimeVal { sec: 0, usec: 0 })
                .unwrap();
        }

        // server clock runs `offset` microseconds ahead of the client clock, with
        // a symmetric one-way network delay
        let offset_us: i64 = 250_000;
        let delay_us: i64 = 5_000;
        let mut server_reply_id: u16 = 0;

        for k in 0..20i64 {
            let client_send_us = (k + 1) * 2_000;
            let mut req = [0u8; 64];
            let n = client
                .poll_transmit(client_send_us, &mut req)
                .expect("client should emit a Time request");

            // request arrives at the server `delay` later, in server time
            let server_arrival_us = client_send_us + delay_us + offset_us;
            let arrival = TimeVal::from_micros(server_arrival_us);
            let (hdr, payload) = req[..n].split_at(Base::BASE_SIZE);
            server
                .handle_event(Event::HeaderReceived(hdr), arrival)
                .unwrap();
            let out = server
                .handle_event(Event::PacketReceived(payload), arrival)
                .unwrap();

            let (id, client_sent, received) = match out {
                SessionOutput::TimeRequest {
                    id,
                    client_sent,
                    received,
                } => (id, client_sent, received),
                _ => panic!("expected TimeRequest"),
            };

            // server sends the reply immediately (same server instant), stamping
            // sent_tv at write time, echoing the client's sent value as latency
            let reply = Time::as_buf(server_reply_id, id, arrival, received, client_sent);
            server_reply_id = server_reply_id.wrapping_add(1);

            // reply lands back at the client `delay` later, in client time
            let client_recv_us = client_send_us + 2 * delay_us;
            let (hdr, payload) = reply.split_at(Base::BASE_SIZE);
            client
                .handle_event(Event::HeaderReceived(hdr), client_recv_us)
                .unwrap();
            client
                .handle_event(Event::PacketReceived(payload), client_recv_us)
                .unwrap();
        }

        assert!(client.synchronized());
        assert_eq!(client.clock_offset(), TimeVal::from_micros(offset_us));
    }
}
