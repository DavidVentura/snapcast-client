use crate::proto::{CodecHeader, CodecMetadata};
use anyhow::Result;
use enum_dispatch::enum_dispatch;

#[cfg(feature = "flac")]
use claxon::frame::FrameReader;

#[cfg(feature = "opus")]
use opus;

#[enum_dispatch(Decode)]
pub enum Decoder {
    #[cfg(feature = "opus")]
    Opus(opus::Decoder),
    PCM(NoOpDecoder),
    #[cfg(feature = "flac")]
    Flac(FlacDecoder),
}

impl Decoder {
    pub fn new(ch: &CodecHeader) -> anyhow::Result<Decoder> {
        match &ch.metadata {
            CodecMetadata::Pcm(_) => Ok(Decoder::PCM(NoOpDecoder {})),

            #[allow(unused_variables)]
            CodecMetadata::Opus(config) => {
                #[cfg(feature = "opus")]
                {
                    let c = match config.channel_count {
                        1 => opus::Channels::Mono,
                        2 => opus::Channels::Stereo,
                        _ => panic!("unsupported channel configuration"),
                    };
                    return Ok(Decoder::Opus(opus::Decoder::new(config.sample_rate, c)?));
                }
                #[cfg(not(feature = "opus"))]
                anyhow::bail!("Opus disabled at build time");
            }
            CodecMetadata::Flac(_buf) => {
                #[cfg(feature = "flac")]
                return Ok(Decoder::Flac(FlacDecoder::new()));
                #[cfg(not(feature = "flac"))]
                anyhow::bail!("Flac disabled at build time");
            }
        }
    }
}

#[enum_dispatch]
pub trait Decode {
    /// Returns total number of samples
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error>;
}

#[cfg(feature = "opus")]
impl Decode for opus::Decoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        // TODO: fec?
        Ok(self.decode(buf, out, false)? * 2)
    }
}

pub struct NoOpDecoder;

impl Decode for NoOpDecoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        // SAFETY: This is safe by design - a no-op decoder passes the data through as-is
        let (_, converted, _) = unsafe { buf.align_to::<i16>() };
        out[0..converted.len()].copy_from_slice(converted);

        Ok(converted.len())
    }
}

pub struct FlacDecoder {
    dec_buf: Vec<i32>,
}
impl FlacDecoder {
    pub fn new() -> FlacDecoder {
        FlacDecoder {
            dec_buf: Vec::with_capacity(2048),
        }
    }
}

#[cfg(feature = "flac")]
impl Decode for FlacDecoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        let mut fr = FrameReader::new(std::io::Cursor::new(buf));
        let mut c = 0;
        loop {
            if let Ok(Some(block)) = fr.read_next_or_eof(&mut self.dec_buf) {
                for (a, b) in block.stereo_samples() {
                    debug_assert!(a <= i16::MAX as i32);
                    debug_assert!(a >= i16::MIN as i32);
                    debug_assert!(b <= i16::MAX as i32);
                    debug_assert!(b >= i16::MIN as i32);
                    out[c + 0] = a as i16;
                    out[c + 1] = b as i16;
                    c += 2;
                }
            } else {
                break;
            }
        }
        Ok(c)
    }
}
