use snapcast_client::proto::{Base, ClientMessage, ServerSettings, Time, TimeVal, WireChunk};
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant, UNIX_EPOCH};
pub struct Server {}

fn handle_client(mut s: TcpStream) -> anyhow::Result<()> {
    let mut hdr_buf = vec![0; Base::BASE_SIZE];
    let mut pkt_buf = vec![0; 9000];
    let time_base = Instant::now();
    let mut timecount: u16 = 0;

    let mut send_fd = s.try_clone()?;
    std::thread::spawn(move || {
        let payload = vec![0; 5760]; // 2880x2 channels; 2880 = 1440 u16 samples = 30ms
        loop {
            std::thread::sleep(Duration::from_millis(30));
            let now = std::time::SystemTime::now();
            let now = TimeVal::from(now.duration_since(UNIX_EPOCH).unwrap());
            let buf = WireChunk {
                timestamp: now,
                payload: &payload,
            }
            .as_buf();
            send_fd.write_all(&buf).unwrap();
        }
    });
    loop {
        s.read_exact(&mut hdr_buf)?;
        let b = Base::from(hdr_buf.as_ref());
        s.read_exact(&mut pkt_buf[0..b.size as usize])?;
        let decoded_m = b.decode_c(&pkt_buf[0..b.size as usize]);
        let recv_ts = TimeVal::from(time_base.elapsed());

        let reply = match decoded_m {
            ClientMessage::Hello(_s) => ServerSettings {
                bufferMs: 500,
                latency: 0,
                muted: false,
                volume: 100,
            }
            .as_buf(),
            ClientMessage::Time(_t) => {
                let now = std::time::SystemTime::now();
                let now = TimeVal::from(now.duration_since(UNIX_EPOCH)?);
                // ???
                let t = Time::as_buf(timecount, now, recv_ts, now);
                timecount = timecount.wrapping_add(1);
                t
            }
        };

        s.write_all(&reply)?;
    }
}
fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;
    for stream in listener.incoming() {
        handle_client(stream?)?;
    }
    Ok(())
}
