mod decoder;
mod playback;
mod proto;

use decoder::{Decode, Decoder};
use playback::{AlsaPlayer, Player, Players};

use std::collections::VecDeque;

use proto::{Base, Server, ServerMessage};

use std::io::prelude::*;
use std::net::TcpStream;

fn main() -> anyhow::Result<()> {
    let srv = Server::new("11:22:33:44:55:66".into(), "framework".into());
    let b = srv.hello();
    let mut s = TcpStream::connect("192.168.2.131:1704")?;
    s.write(&b)?;

    let mut dec: Option<Decoder> = None;
    // >= (960 * 2) for OPUS
    // == 2880 for PCM
    let mut samples_out = vec![0; 2880];

    // localhost MTU is pretty large )
    let mut pkt_buf = vec![0; 14500];
    let mut buf_samples = VecDeque::new();
    let mut enough_to_start = false;

    let player: Players = Players::from(AlsaPlayer::new()?);
    loop {
        // assumes multiple packets per read, but never half a packet, will panic
        let mut remaining_bytes = s.read(&mut pkt_buf)?;
        let mut read_bytes = 0;

        while remaining_bytes > 0 {
            let b = Base::from(&pkt_buf[read_bytes..]);
            remaining_bytes -= b.total_size();
            read_bytes += b.total_size();

            let decoded_m = b.decode();
            match decoded_m {
                ServerMessage::CodecHeader(ch) => _ = dec.insert(Decoder::new(ch)?),
                ServerMessage::WireChunk(wc) => {
                    if let Some(ref mut dec) = dec {
                        let s = dec.decode_sample(wc.payload, &mut samples_out)?;

                        buf_samples.push_back(samples_out[0..s].to_vec());
                        if buf_samples.len() > 2 {
                            enough_to_start = true;
                        }
                        if enough_to_start {
                            if let Some(buffered_sample) = buf_samples.pop_front() {
                                player.play()?;
                                player.write(&buffered_sample)?;
                            }
                        }
                    }
                }
                other => println!("unhandled: {:?}", other),
            }
        }
    }
}
