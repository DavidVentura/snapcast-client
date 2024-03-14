mod decoder;
mod playback;
mod proto;

use decoder::{Decode, Decoder};
#[cfg(feature = "alsa")]
use playback::Alsa;
#[cfg(feature = "pulse")]
use playback::Pulse;
use playback::{File, Player, Players, Tcp};

use std::collections::VecDeque;

use proto::{Base, Server, ServerMessage, Time, TimeVal};

use std::io::prelude::*;
use std::net::TcpStream;
use std::sync::{mpsc, Arc, Mutex};
use std::time;

fn main() -> anyhow::Result<()> {
    let mut s = TcpStream::connect("192.168.2.131:1704")?;
    s.set_nodelay(true)?;

    let srv = Server::new("11:22:33:44:55:66".into(), "framework".into());
    {
        let b = srv.hello();
        s.write_all(&b)?;
    }

    let mut send_side = s.try_clone()?;

    let time_base_c = time::Instant::now();

    let now = time_base_c.elapsed();
    let tv = TimeVal {
        sec: now.as_secs() as i32, // allows for 68 years of uptime
        usec: now.subsec_micros() as i32,
    };
    println!("my delta {now:?} {tv:?}");
    let t = Time::as_buf(0, tv, tv, tv);
    send_side.write_all(&t).unwrap();

    std::thread::spawn(move || {
        let mut i: u16 = 1;
        loop {
            std::thread::sleep(time::Duration::from_millis(100));
            let now = time_base_c.elapsed();
            let tv = TimeVal {
                sec: now.as_secs() as i32, // allows for 68 years of uptime
                usec: now.subsec_micros() as i32,
            };
            //println!("my delta {:?}", now);
            let t = Time::as_buf(i, tv, tv, tv);
            send_side.write_all(&t).unwrap();
            i = i.wrapping_add(1);
        }
    });

    let dec: Arc<Mutex<Option<Decoder>>> = Arc::new(Mutex::new(None));
    let dec_2 = dec.clone();

    let player: Arc<Mutex<Option<Players>>> = Arc::new(Mutex::new(None));
    let player_2 = player.clone();

    let mut buffer_ms = TimeVal {
        sec: 0,
        usec: 999_999,
    };
    let mut local_latency = TimeVal { sec: 0, usec: 0 };
    let mut tbase_adj = TimeVal { sec: 0, usec: 0 }; // t_s - t_c

    let (sample_tx, sample_rx) = mpsc::channel::<(TimeVal, Vec<u8>)>();
    std::thread::spawn(move || {
        // >= (960 * 2) for OPUS
        // >= 2880 for PCM
        let mut samples_out = vec![0; 4096];
        while let Ok((client_audible_ts, samples)) = sample_rx.recv() {
            // Guard against chunks coming before the decoder is initialized
            if let Some(ref mut dec) = *dec.lock().unwrap() {
                let decoded_sample_c = dec.decode_sample(&samples, &mut samples_out).unwrap();
                let sample = &samples_out[0..decoded_sample_c];
                if let Some(ref mut p) = *player.lock().unwrap() {
                    p.play().unwrap();
                    p.write(sample).unwrap();
                }
            }
        }
    });

    let mut hdr_buf = vec![0; 26];
    // localhost MTU is pretty large )
    let mut pkt_buf = vec![0; 6000];
    loop {
        s.read_exact(&mut hdr_buf)?;
        let b = Base::from(hdr_buf.as_slice());
        s.read_exact(&mut pkt_buf[0..b.size as usize])?;

        let decoded_m = b.decode(&pkt_buf[0..b.size as usize]);
        match decoded_m {
            ServerMessage::CodecHeader(ch) => {
                _ = dec_2.lock().unwrap().insert(Decoder::new(&ch)?);

                #[cfg(feature = "alsa")]
                {
                    let p: Players = Players::from(Alsa::new(ch.metadata.rate())?);
                    _ = player_2.lock().unwrap().insert(p);
                }
                #[cfg(feature = "pulse")]
                {
                    let p: Players = Players::from(Pulse::new(ch.metadata.rate())?);
                    _ = player_2.lock().unwrap().insert(p);
                }
                #[cfg(not(any(feature = "alsa", feature = "pulse")))]
                {
                    println!("Compiled without support for pulse/alsa, outputting to TCP");
                    let p: Players = Players::from(Tcp::new("127.0.0.1:12345")?);
                    _ = player_2.lock().unwrap().insert(p);
                    //println!("Compiled without support for pulse/alsa, outputting to out.pcm");
                    //let p: Players = Players::from(File::new(std::path::Path::new("out.pcm"))?);
                    //_ = player_2.lock().unwrap().insert(p);
                }
            }
            ServerMessage::WireChunk(wc) => {
                let t_c = TimeVal::from(time_base_c.elapsed());
                let audible_at = t_c + tbase_adj + buffer_ms - local_latency;
                sample_tx.send((audible_at, wc.payload.to_vec()));
            }

            ServerMessage::ServerSettings(s) => {
                buffer_ms = TimeVal::from_millis(s.bufferMs as i32);
                local_latency = TimeVal::from_millis(s.latency as i32);
            }
            ServerMessage::Time(t) => {
                // TODO median for these 2
                // time_base_s == t.latency;
                tbase_adj = t.latency - time_base_c.elapsed().into();
            }
        }
    }
}
