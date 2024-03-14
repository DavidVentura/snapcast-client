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
    let time_zero = time_base_c.elapsed();

    std::thread::spawn(move || {
        let mut i: u32 = 0;
        loop {
            let now = time_base_c.elapsed();
            let tv = TimeVal {
                sec: now.as_secs() as i32, // allows for 68 years of uptime
                usec: now.subsec_micros() as i32,
            };
            let t = Time::as_buf(i as u16, tv, tv, tv);
            send_side.write_all(&t).unwrap();
            i = i.wrapping_add(1); // wraps every 136 years

            // on startup, calibrate latency 50 times
            let sleep_len = if i > 50 { 1000 } else { 1 };
            std::thread::sleep(time::Duration::from_millis(sleep_len));
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

        let mut player_lat_ms: u16 = 0;
        while let Ok((client_audible_ts, samples)) = sample_rx.recv() {
            let mut valid = true;
            loop {
                let remaining = client_audible_ts - time_base_c.elapsed().into();
                if remaining.sec < 0 {
                    valid = false;
                    break;
                }

                if remaining.millis().unwrap() <= player_lat_ms {
                    break;
                }
                std::thread::sleep(time::Duration::from_millis(1));
            }

            if !valid {
                println!("aaa in the past");
                continue;
            }

            // Guard against chunks coming before the decoder is initialized
            let Some(ref mut dec) = *dec.lock().unwrap() else {
                continue;
            };
            let Some(ref mut p) = *player.lock().unwrap() else {
                continue;
            };
            player_lat_ms = p.latency_ms().unwrap();
            let decoded_sample_c = dec.decode_sample(&samples, &mut samples_out).unwrap();
            let sample = &samples_out[0..decoded_sample_c];
            p.play().unwrap();
            p.write(sample).unwrap();
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
                let t_s = wc.timestamp;
                let t_c = t_s - tbase_adj;
                let audible_at = t_c + buffer_ms - local_latency;
                sample_tx.send((audible_at, wc.payload.to_vec()))?;
            }

            ServerMessage::ServerSettings(s) => {
                buffer_ms = TimeVal::from_millis(s.bufferMs as i32);
                local_latency = TimeVal::from_millis(s.latency as i32);
                // TODO volume
            }
            ServerMessage::Time(t) => {
                // TODO median for these 2
                // time_base_s == t.latency;
                tbase_adj = t.latency - time_zero.into();
            }
        }
    }
}
