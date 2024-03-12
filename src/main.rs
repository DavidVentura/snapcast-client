mod proto;

use opus::{Channels, Decoder};

use proto::{Base, CodecMetadata, Server, ServerMessage};

use std::io::prelude::*;
use std::net::TcpStream;

use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

use byte_slice_cast::*;

fn main() {
    println!("Hello, world!");
    let srv = Server::new("11:22:33:44:55:66".into(), "framework".into());
    let b = srv.hello();
    let mut s = TcpStream::connect("127.0.0.1:1704").unwrap();
    s.write(&b).unwrap();

    let mut dec: Option<Decoder> = None;

    let mut samples_out = vec![0; 35000];
    let spec = Spec {
        format: Format::S16NE,
        channels: 2,
        rate: 48000,
    };
    let pulseaudio = Simple::new(
        None,                // Use the default server
        "FooApp",            // Our application’s name
        Direction::Playback, // We want a playback stream
        None,                // Use the default device
        "Music",             // Description of our stream
        &spec,               // Our sample format
        None,                // Use default channel map
        None,                // Use default buffering attributes
    )
    .unwrap();

    let mut pkt_buf = vec![0; 1500];

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
                ServerMessage::CodecHeader(ch) => match ch.metadata {
                    CodecMetadata::Opaque(_) => todo!(),
                    CodecMetadata::Opus(config) => {
                        println!("{config:?}");
                        let c = match config.channel_count {
                            1 => Channels::Mono,
                            2 => Channels::Stereo,
                            _ => panic!("unsupported channel configuration"),
                        };
                        let d = Decoder::new(config.sample_rate, c).unwrap();
                        _ = dec.insert(d);
                    }
                },
                ServerMessage::WireChunk(wc) => {
                    match &mut dec {
                        Some(dec) => {
                            let decoded_samples =
                                dec.decode(wc.payload, &mut samples_out, false).unwrap();
                            // TODO: fec?
                            println!("decoded samples {}", decoded_samples);
                            let as_u8 = samples_out.as_byte_slice().as_slice_of::<u8>().unwrap();
                            // TODO: *2 is not great
                            // mb use get_nb_samples
                            //pulseaudio.write(&as_u8[0..decoded_samples * 2]).unwrap();
                            pulseaudio.write(&as_u8).unwrap();
                        }
                        None => (),
                    };
                }
                other => println!("unhandled: {:?}", other),
            }
        }
    }
}
