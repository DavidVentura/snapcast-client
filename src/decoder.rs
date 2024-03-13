use crate::proto::{CodecHeader, CodecMetadata};
use anyhow::Result;
use opus;

pub(crate) enum Decoder {
    Opus(opus::Decoder),
    PCM(NoOpDecoder),
}

impl Decoder {
    pub fn new(ch: CodecHeader) -> Decoder {
        match ch.metadata {
            CodecMetadata::Opaque(header) => {
                // TODO: discriminate opaque types
                Decoder::PCM(NoOpDecoder {})
            }
            CodecMetadata::Opus(config) => {
                let c = match config.channel_count {
                    1 => opus::Channels::Mono,
                    2 => opus::Channels::Stereo,
                    _ => panic!("unsupported channel configuration"),
                };
                Decoder::Opus(opus::Decoder::new(config.sample_rate, c).unwrap())
            }
        }
    }
}

pub(crate) trait Decode {
    /// Returns number of samples
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error>;
}

impl Decode for Decoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        match self {
            Decoder::Opus(o) => o.decode_sample(buf, out),
            Decoder::PCM(p) => p.decode_sample(buf, out),
        }
    }
}

impl Decode for opus::Decoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        // TODO: fec?
        Ok(self.decode(buf, out, false)?)
    }
}

pub(crate) struct NoOpDecoder;

impl Decode for NoOpDecoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        let (_, converted, _) = unsafe { buf.align_to::<i16>() };
        out.copy_from_slice(converted);
        Ok(converted.len())
    }
}
