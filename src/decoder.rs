use crate::proto::{CodecHeader, CodecMetadata};
use anyhow::Result;
use enum_dispatch::enum_dispatch;
use opus;

#[enum_dispatch(Decode)]
pub(crate) enum Decoder {
    Opus(opus::Decoder),
    PCM(NoOpDecoder),
}

impl Decoder {
    pub fn new(ch: CodecHeader) -> anyhow::Result<Decoder> {
        match ch.metadata {
            CodecMetadata::Opaque(header) => {
                // TODO: discriminate opaque types
                Ok(Decoder::PCM(NoOpDecoder {}))
            }
            CodecMetadata::Opus(config) => {
                let c = match config.channel_count {
                    1 => opus::Channels::Mono,
                    2 => opus::Channels::Stereo,
                    _ => panic!("unsupported channel configuration"),
                };
                Ok(Decoder::Opus(opus::Decoder::new(config.sample_rate, c)?))
            }
        }
    }
}

#[enum_dispatch]
pub(crate) trait Decode {
    /// Returns total number of samples
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error>;
}

impl Decode for opus::Decoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        // TODO: fec?
        Ok(self.decode(buf, out, false)? * 2)
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
