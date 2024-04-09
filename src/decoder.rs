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
                anyhow::bail!("Opus disabled at build time");
            }
            CodecMetadata::Flac(buf) => {
                #[cfg(feature = "flac")]
                return Ok(Decoder::Flac(FlacDecoder::new()));
                anyhow::bail!("Flac disabled at build time");
            }
            _ => anyhow::bail!("Don't know how to handle {:?}", ch.metadata),
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

pub struct FlacDecoder {}
impl FlacDecoder {
    pub fn new() -> FlacDecoder {
        FlacDecoder {}
    }
}

#[cfg(feature = "flac")]
impl Decode for FlacDecoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        let mut fr = FrameReader::new(std::io::Cursor::new(buf));
        let v = Vec::new();
        let block = fr.read_next_or_eof(v).unwrap().unwrap();
        let mut c = 0;
        for (i, (a, b)) in block.stereo_samples().enumerate() {
            out[i * 2 + 0] = (a / 2) as i16;
            out[i * 2 + 1] = (b / 2) as i16;
            c = i;
        }
        Ok(c * 2)
    }
}
