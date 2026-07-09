use std::collections::HashMap;
use std::io::prelude::*;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceInfo};

use snapcast_client::framing::{Action, Event};
use snapcast_client::proto::{
    Base, CodecHeader, CodecMetadata, OpusMetadata, ServerSettings, Time, TimeVal, WireChunk,
};
use snapcast_client::server::{ServerSession, SessionOutput};

/// The one server time base. A single [`Instant`] captured at startup feeds every
/// `WireChunk.timestamp` and every Time-reply field, so client offset math lines up.
#[derive(Copy, Clone)]
pub struct ServerClock {
    base: Instant,
}

impl ServerClock {
    pub fn new() -> ServerClock {
        ServerClock {
            base: Instant::now(),
        }
    }
    pub fn now_us(&self) -> i64 {
        self.base.elapsed().as_micros() as i64
    }
    pub fn now_tv(&self) -> TimeVal {
        TimeVal::from_micros(self.now_us())
    }
}

pub struct EncodedChunk {
    pub timestamp: TimeVal,
    pub data: Vec<u8>,
}

/// A message queued for one client's writer thread. Chunks are droppable under
/// backpressure; everything else is a control message and must not be dropped.
pub enum Outbound {
    Chunk(Arc<EncodedChunk>),
    Settings(ServerSettings),
    CodecHeader,
    TimeReply {
        refers_to: u16,
        client_sent: TimeVal,
        received: TimeVal,
    },
}

type ClientId = u64;

/// ~16 slots ≈ 320ms of 20ms chunks: enough to ride out a brief stall, small
/// enough that a wedged client is noticed within a fraction of the server buffer.
const CHANNEL_SLOTS: usize = 16;

struct ClientHandle {
    tx: SyncSender<Outbound>,
    /// A clone of the socket, used only to force-disconnect a wedged client.
    sock: TcpStream,
    full_since: Option<Instant>,
    dropped: u64,
}

struct Inner {
    clients: HashMap<ClientId, ClientHandle>,
    volume: u8,
    next_id: ClientId,
}

pub struct Registry {
    inner: Mutex<Inner>,
    buffer_ms: u32,
}

impl Registry {
    pub fn new(buffer_ms: u32, initial_volume: u8) -> Registry {
        Registry {
            inner: Mutex::new(Inner {
                clients: HashMap::new(),
                volume: initial_volume,
                next_id: 0,
            }),
            buffer_ms,
        }
    }

    pub fn current_settings(&self) -> ServerSettings {
        let inner = self.inner.lock().unwrap();
        ServerSettings {
            bufferMs: self.buffer_ms,
            latency: 0,
            muted: false,
            volume: inner.volume,
        }
    }

    fn register(&self, tx: SyncSender<Outbound>, sock: TcpStream) -> ClientId {
        let mut inner = self.inner.lock().unwrap();
        let id = inner.next_id;
        inner.next_id += 1;
        inner.clients.insert(
            id,
            ClientHandle {
                tx,
                sock,
                full_since: None,
                dropped: 0,
            },
        );
        id
    }

    fn unregister(&self, id: ClientId) {
        self.inner.lock().unwrap().clients.remove(&id);
    }

    fn disconnect(&self, id: ClientId) {
        if let Some(h) = self.inner.lock().unwrap().clients.remove(&id) {
            log::warn!(
                "disconnecting wedged client {id} after {} dropped chunks",
                h.dropped
            );
            let _ = h.sock.shutdown(Shutdown::Both);
        }
    }

    /// Fan a chunk out to every client. Chunks are dropped for any client whose
    /// queue is full (it will notice the gap and expire it); a client whose queue
    /// stays full longer than twice the server buffer is force-disconnected.
    pub fn broadcast_chunk(&self, chunk: Arc<EncodedChunk>) {
        let limit = Duration::from_millis((self.buffer_ms * 2) as u64);
        let mut wedged = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap();
            for (id, h) in inner.clients.iter_mut() {
                match h.tx.try_send(Outbound::Chunk(chunk.clone())) {
                    Ok(()) => h.full_since = None,
                    Err(TrySendError::Full(_)) => {
                        h.dropped += 1;
                        let since = *h.full_since.get_or_insert_with(Instant::now);
                        if since.elapsed() > limit {
                            wedged.push(*id);
                        }
                    }
                    Err(TrySendError::Disconnected(_)) => wedged.push(*id),
                }
            }
        }
        for id in wedged {
            self.disconnect(id);
        }
    }

    /// Update the stored volume and push fresh settings to every client. Control
    /// messages use a blocking send outside the lock, so they are never dropped.
    pub fn broadcast_settings(&self, volume: u8) {
        let (settings, txs) = {
            let mut inner = self.inner.lock().unwrap();
            inner.volume = volume;
            let settings = ServerSettings {
                bufferMs: self.buffer_ms,
                latency: 0,
                muted: false,
                volume,
            };
            let txs: Vec<SyncSender<Outbound>> =
                inner.clients.values().map(|h| h.tx.clone()).collect();
            (settings, txs)
        };
        for tx in txs {
            let _ = tx.send(Outbound::Settings(settings.clone()));
        }
    }
}

fn opus_codec_header() -> CodecHeader<'static> {
    CodecHeader {
        codec: "opus",
        metadata: CodecMetadata::Opus(OpusMetadata {
            sample_rate: 48_000,
            bit_depth: 16,
            channel_count: 2,
        }),
    }
}

/// Advertise the stream port over mDNS as `_snapcast._tcp` so clients can find us
/// without a hardcoded address. The returned daemon must be kept alive; dropping
/// it withdraws the advertisement.
pub fn advertise(device_name: &str, port: u16) -> anyhow::Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new()?;
    let host = format!("{}.local.", device_name.replace(' ', "-"));
    let service = ServiceInfo::new(
        "_snapcast._tcp.local.",
        device_name,
        &host,
        "",
        port,
        &[] as &[(&str, &str)],
    )?
    .enable_addr_auto();
    mdns.register(service)?;
    log::info!("advertising _snapcast._tcp on port {port} as '{device_name}'");
    Ok(mdns)
}

pub fn accept_loop(listener: TcpListener, registry: Arc<Registry>, clock: ServerClock) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                log::warn!("accept failed: {e}");
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let registry = registry.clone();
        std::thread::spawn(move || {
            if let Err(e) = run_reader(stream, registry, clock) {
                log::info!("client connection ended: {e}");
            }
        });
    }
}

/// Reads from one client, driving its [`ServerSession`], registering it on Hello
/// and queueing Time replies. Unregisters on any error so the writer tears down.
fn run_reader(
    stream: TcpStream,
    registry: Arc<Registry>,
    clock: ServerClock,
) -> anyhow::Result<()> {
    let mut my_id: Option<ClientId> = None;
    let mut my_tx: Option<SyncSender<Outbound>> = None;
    let result = reader_loop(&stream, &registry, clock, &mut my_id, &mut my_tx);
    if let Some(id) = my_id {
        registry.unregister(id);
    }
    let _ = stream.shutdown(Shutdown::Both);
    result
}

fn reader_loop(
    stream: &TcpStream,
    registry: &Arc<Registry>,
    clock: ServerClock,
    my_id: &mut Option<ClientId>,
    my_tx: &mut Option<SyncSender<Outbound>>,
) -> anyhow::Result<()> {
    let mut session = ServerSession::new();
    let mut reader = stream;
    let mut hdr = vec![0u8; Base::BASE_SIZE];
    let mut pkt = vec![0u8; 8192];

    loop {
        match session.next_action() {
            Action::ReadHeader => {
                reader.read_exact(&mut hdr)?;
                session.handle_event(Event::HeaderReceived(&hdr), clock.now_tv())?;
            }
            Action::ReadPacket(size) => {
                let size = size as usize;
                if size > pkt.len() {
                    pkt.resize(size, 0);
                }
                reader.read_exact(&mut pkt[0..size])?;
                let out = session.handle_event(Event::PacketReceived(&pkt[0..size]), clock.now_tv())?;
                match out {
                    SessionOutput::None => {}
                    SessionOutput::Hello(h) => {
                        log::info!("client hello from {} ({})", h.HostName, h.MAC);
                        let (tx, rx) = sync_channel::<Outbound>(CHANNEL_SLOTS);
                        let write_half = stream.try_clone()?;
                        std::thread::spawn(move || run_writer(write_half, rx, clock));
                        let id = registry.register(tx.clone(), stream.try_clone()?);
                        // a fresh client needs settings (with current volume) then the codec
                        tx.send(Outbound::Settings(registry.current_settings()))?;
                        tx.send(Outbound::CodecHeader)?;
                        *my_id = Some(id);
                        *my_tx = Some(tx);
                    }
                    SessionOutput::TimeRequest {
                        id,
                        client_sent,
                        received,
                    } => {
                        if let Some(tx) = my_tx.as_ref() {
                            tx.send(Outbound::TimeReply {
                                refers_to: id,
                                client_sent,
                                received,
                            })?;
                        }
                    }
                }
            }
        }
    }
}

/// Serializes queued messages onto the socket. Time replies get their `sent_tv`
/// stamped here, at the last possible moment before the write.
fn run_writer(mut w: TcpStream, rx: Receiver<Outbound>, clock: ServerClock) {
    let codec_header = opus_codec_header();
    let mut id: u16 = 0;
    while let Ok(msg) = rx.recv() {
        let buf = match msg {
            Outbound::Chunk(ec) => WireChunk {
                timestamp: ec.timestamp,
                payload: &ec.data,
            }
            .as_buf(id, clock.now_tv()),
            Outbound::Settings(s) => s.as_buf(id, clock.now_tv()),
            Outbound::CodecHeader => codec_header.as_buf(id, clock.now_tv()),
            Outbound::TimeReply {
                refers_to,
                client_sent,
                received,
            } => Time::as_buf(id, refers_to, clock.now_tv(), received, client_sent),
        };
        id = id.wrapping_add(1);
        if w.write_all(&buf).is_err() {
            break;
        }
    }
    let _ = w.shutdown(Shutdown::Both);
}
