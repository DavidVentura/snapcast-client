mod decoder;
mod playback;
mod proto;

use decoder::{Decode, Decoder};

use std::collections::VecDeque;

use proto::{Base, Server, ServerMessage};

use std::io::prelude::*;
use std::net::TcpStream;

fn main() {
    let srv = Server::new("11:22:33:44:55:66".into(), "framework".into());
    let b = srv.hello();
    let mut s = TcpStream::connect("192.168.2.131:1704").unwrap();
    s.write(&b).unwrap();

    let mut dec: Option<Decoder> = None;
    let mut samples_out = vec![0; 960 * 2];
    let mut pkt_buf = vec![0; 14500];
    let mut buf_samples = VecDeque::new();
    let mut enough_to_start = false;

    let player = playback::alsa::AlsaPlayer::new();
    //let player = playback::pulse::PulsePlayer::new();
    loop {
        // assumes multiple packets per read, but never half a packet, will panic
        let mut remaining_bytes = s.read(&mut pkt_buf).unwrap();
        let mut read_bytes = 0;

        while remaining_bytes > 0 {
            let b = Base::from(&pkt_buf[read_bytes..]);
            remaining_bytes -= b.total_size();
            read_bytes += b.total_size();

            let decoded_m = b.decode();
            match decoded_m {
                ServerMessage::CodecHeader(ch) => _ = dec.insert(Decoder::new(ch)),
                ServerMessage::WireChunk(wc) => {
                    if let Some(ref mut dec) = dec {
                        dec.decode_sample(wc.payload, &mut samples_out).unwrap();

                        buf_samples.push_back(samples_out.clone());
                        if buf_samples.len() > 10 {
                            enough_to_start = true;
                        }
                        if enough_to_start {
                            let buffered_sample = buf_samples.pop_front().unwrap();
                            player.play();
                            player.write(&buffered_sample);
                        }
                    }
                }
                other => println!("unhandled: {:?}", other),
            }
        }
    }
}
