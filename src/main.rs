mod client;
mod decoder;
mod framing;
mod playback;
mod proto;

use client::{Client, Message};
#[cfg(feature = "alsa")]
use playback::Alsa;
#[cfg(feature = "pulse")]
use playback::Pulse;
use playback::{File, Player, Players, Tcp};
use proto::{CodecHeader, CodecMetadata, TimeVal};

use clap::Parser;
use decoder::{Decode, Decoder};

use std::sync::{mpsc, Arc, Mutex};
use std::time;

#[derive(clap::ValueEnum, Debug, Copy, Clone)]
enum PlayerBackend {
    #[cfg(feature = "alsa")]
    Alsa,
    #[cfg(feature = "pulse")]
    Pulse,
    TCP,
    File,
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, value_enum)]
    backend: PlayerBackend,

    #[arg(short, long, default_value = "192.168.2.183:1704")]
    server: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = Client::new("11:22:33:44:55:66".into(), "framework".into());
    let mut client = client.connect(args.server.as_str())?;
    let time_base_c = client.time_base();

    let dec: Arc<Mutex<Option<Decoder>>> = Arc::new(Mutex::new(None));
    let dec_2 = dec.clone();

    let player: Arc<Mutex<Option<Players>>> = Arc::new(Mutex::new(None));
    let player_2 = player.clone();

    let (sample_tx, sample_rx) = mpsc::channel::<(TimeVal, Vec<u8>)>();
    std::thread::spawn(move || handle_samples(sample_rx, time_base_c, player, dec));

    loop {
        let in_sync = client.synchronized();
        let msg = client.tick()?;
        match msg {
            Message::CodecHeader(ch) => {
                #[allow(unreachable_patterns)]
                let d = match &ch.metadata {
                    CodecMetadata::Pcm(_) => Decoder::new_pcm(),
                    #[cfg(feature = "flac")]
                    CodecMetadata::Flac(_) => Decoder::new_flac(),
                    #[cfg(feature = "opus")]
                    CodecMetadata::Opus(cfg) => {
                        Decoder::new_opus(cfg, Box::leak(Box::new_uninit()))?
                    }
                    other => anyhow::bail!("codec disabled at build time: {other:?}"),
                };
                _ = dec_2.lock().unwrap().insert(d);
                let p = make_player(args.backend, &ch)?;
                _ = player_2.lock().unwrap().insert(p);
            }
            Message::WireChunk(wc, audible_at) => {
                // before the offset buffer fills, audible_at is computed from a
                // bogus clock offset; forwarding those would schedule playback
                // wildly in the future
                if in_sync {
                    sample_tx.send((audible_at, wc.payload.to_vec()))?;
                }
            }

            Message::ServerSettings(_v) => {
                // TODO volume
            }
            _ => (),
        }
    }
}

fn handle_samples(
    sample_rx: mpsc::Receiver<(TimeVal, Vec<u8>)>,
    time_base_c: time::Instant,
    player: Arc<Mutex<Option<Players>>>,
    dec: Arc<Mutex<Option<Decoder>>>,
) {
    // >= (960 * 2) for OPUS
    // >= 2880 for PCM
    // >= 4600 for FLAC
    let mut samples_out = vec![0; 4700];

    let mut player_lat_ms: u16 = 1;
    while let Ok((client_audible_ts, samples)) = sample_rx.recv() {
        let remaining = client_audible_ts - time_base_c.elapsed().into();
        if remaining.sec < 0 {
            println!("aaa in the past");
            continue;
        }

        // sleep until the chunk is due, leaving the player's own latency of lead
        let remaining_us = remaining.to_micros();
        let lead_us = player_lat_ms as i64 * 1000;
        if remaining_us > lead_us {
            std::thread::sleep(time::Duration::from_micros((remaining_us - lead_us) as u64));
        }

        // Guard against chunks coming before the decoder is initialized
        let Some(ref mut dec) = *dec.lock().unwrap() else {
            continue;
        };
        let Some(ref mut p) = *player.lock().unwrap() else {
            continue;
        };
        // Backends with 0ms of buffer (file, tcp) otherwise behave erratically
        player_lat_ms = std::cmp::max(1, p.latency_ms().unwrap());
        let decoded_sample_c = dec.decode_sample(&samples, &mut samples_out).unwrap();
        let mut sample = &mut samples_out[0..decoded_sample_c];
        p.play().unwrap();
        p.write(&mut sample).unwrap();
    }
}

fn make_player(b: PlayerBackend, ch: &CodecHeader) -> anyhow::Result<Players> {
    match b {
        #[cfg(feature = "alsa")]
        PlayerBackend::Alsa => Ok(Players::from(Alsa::new(ch.metadata.rate())?)),
        #[cfg(feature = "pulse")]
        PlayerBackend::Pulse => Ok(Players::from(Pulse::new(ch.metadata.rate())?)),
        PlayerBackend::TCP => Ok(Players::from(Tcp::new(
            "127.0.0.1:12345",
            ch.metadata.rate(),
        )?)),
        PlayerBackend::File => Ok(Players::from(File::new(
            std::path::Path::new("out.pcm"),
            ch.metadata.rate(),
        )?)),
    }
}
