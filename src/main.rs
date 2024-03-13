mod proto;

use decoder::{Decode, Decoder};

use std::collections::VecDeque;

use proto::{Base, Server, ServerMessage};

use std::io::prelude::*;
use std::net::TcpStream;

use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};

//use libpulse_binding::sample::{Format, Spec};
//use libpulse_binding::stream::Direction;
//use libpulse_simple_binding::Simple;

//use byte_slice_cast::*;

mod decoder;

fn main() {
    let srv = Server::new("11:22:33:44:55:66".into(), "framework".into());
    let b = srv.hello();
    let mut s = TcpStream::connect("192.168.2.131:1704").unwrap();
    s.write(&b).unwrap();

    let mut dec: Option<Decoder> = None;

    let mut samples_out = vec![0; 960 * 2];
    /*
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
    */

    let mut pkt_buf = vec![0; 14500];

    // Open default playback device
    let pcm = PCM::new("default", Direction::Playback, false).unwrap();

    // Set hardware parameters: 48000 Hz / Stereo / 16 bit
    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(2).unwrap();
    hwp.set_rate(48000, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    // Make sure we don't start the stream too early
    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    swp.set_start_threshold(hwp.get_buffer_size().unwrap())
        .unwrap();
    pcm.sw_params(&swp).unwrap();

    //let mut gd = GenericDecoder::new();
    //let sr = StreamReader::<GenericDecoder>::new(gd);
    // let mut f = std::fs::File::create("out.wav").unwrap();
    let mut buf_samples = VecDeque::new();
    let mut enough_to_start = false;
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
                    /*
                    let (_, converted, _) = unsafe { wc.payload.align_to::<i16>() };
                    io.writei(converted).unwrap();
                    if pcm.state() != State::Running {
                        pcm.start().unwrap()
                    };
                    */
                    //pulseaudio.write(wc.payload).unwrap();
                    //println!("{}", wc.payload.len());
                    //f.write(wc.payload).unwrap();

                    match &mut dec {
                        Some(dec) => {
                            dec.decode_sample(wc.payload, &mut samples_out).unwrap();

                            buf_samples.push_back(samples_out.clone());
                            if buf_samples.len() > 10 {
                                enough_to_start = true;
                            }
                            if enough_to_start {
                                let buffered_sample = buf_samples.pop_front().unwrap();
                                io.writei(&buffered_sample).unwrap();
                                if pcm.state() != State::Running {
                                    pcm.start().unwrap();
                                }
                            }
                            //let as_u8 = samples_out.as_byte_slice().as_slice_of::<u8>().unwrap();
                            // TODO: *2 is not great
                            // mb use get_nb_samples
                            //pulseaudio.write(&as_u8[0..decoded_samples * 2]).unwrap();
                            //pulseaudio.write(&as_u8).unwrap();
                            //f.write_all(as_u8).unwrap();
                        }
                        None => (),
                    };
                }
                other => (), //println!("unhandled: {:?}", other),
            }
        }
    }
}
