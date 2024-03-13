mod decoder;
mod playback;
mod proto;

use decoder::{Decode, Decoder};
#[cfg(feature = "alsa")]
use playback::Alsa;
use playback::{File, Player, Players};

use std::collections::VecDeque;

use proto::{Base, Server, ServerMessage, Time, TimeVal};

use std::io::prelude::*;
use std::net::TcpStream;
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

    let start = time::Instant::now();

    std::thread::spawn(move || {
        let mut i: u16 = 0;
        loop {
            std::thread::sleep(time::Duration::from_millis(100));
            let now = start.elapsed();
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

    let mut dec: Option<Decoder> = None;

    // >= (960 * 2) for OPUS
    // >= 2880 for PCM
    let mut samples_out = vec![0; 4096];

    let mut hdr_buf = vec![0; 26];
    // localhost MTU is pretty large )
    let mut pkt_buf = vec![0; 6000];
    let mut buf_samples = VecDeque::new();
    let mut enough_to_start = false;

    let mut player: Option<Players> = None;

    let mut sample_goal = 0;
    let mut buffer_len = 999; // default
    loop {
        s.read_exact(&mut hdr_buf)?;
        let b = Base::from(hdr_buf.as_slice());
        s.read_exact(&mut pkt_buf[0..b.size as usize])?;

        let decoded_m = b.decode(&pkt_buf[0..b.size as usize]);
        match decoded_m {
            ServerMessage::CodecHeader(ch) => {
                _ = dec.insert(Decoder::new(&ch)?);

                #[cfg(feature = "alsa")]
                {
                    let p: Players = Players::from(Alsa::new(ch.metadata.rate())?);
                    _ = player.insert(p);
                }
                #[cfg(not(any(feature = "alsa", feature = "pulse")))]
                {
                    println!("Compiled without support for pulse/alsa, outputting to out.pcm");
                    let p: Players = Players::from(File::new(std::path::Path::new("out.pcm"))?);
                    _ = player.insert(p);
                }
                sample_goal = buffer_len / 1000 * ch.metadata.rate();
                println!(
                    "buffer goal: {buffer_len}, need samples: {sample_goal}\n{:?}",
                    ch
                );
            }
            ServerMessage::WireChunk(wc) => {
                //println!("wc ts {:?}", wc.timestamp);
                // Guard against chunks coming before the decoder is initialized
                if let Some(ref mut dec) = dec {
                    let s = dec.decode_sample(wc.payload, &mut samples_out)?;

                    buf_samples.push_back(samples_out[0..s].to_vec());

                    // assuming all sample blocks have the same len, otherwise need to extend
                    if (buf_samples.len() - 1) * s > sample_goal {
                        enough_to_start = true;
                    }

                    if enough_to_start {
                        if let Some(buffered_sample) = buf_samples.pop_front() {
                            if let Some(ref mut p) = player {
                                p.play()?;
                                p.write(&buffered_sample)?;
                            }
                        }
                    }
                }
            }

            ServerMessage::ServerSettings(s) => {
                buffer_len = s.bufferMs as usize;
            }
            other => (), //println!("unhandled: {:?}", other),
        }
    }
}
