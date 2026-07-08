#[cfg(feature = "opus")]
use crate::proto::OpusMetadata;
#[cfg(feature = "opus")]
use anyhow::Context;
use anyhow::Result;
use enum_dispatch::enum_dispatch;

#[cfg(feature = "flac")]
use claxon::frame::FrameReader;

#[cfg(feature = "opus")]
use opus_embedded;

#[cfg(feature = "opus")]
pub struct OpusDecoder<'a>(&'a mut opus_embedded::Decoder);

#[enum_dispatch(Decode)]
pub enum Decoder<'a> {
    #[cfg(feature = "opus")]
    Opus(OpusDecoder<'a>),
    PCM(NoOpDecoder),
    #[cfg(feature = "flac")]
    Flac(FlacDecoder),
}

impl<'a> Decoder<'a> {
    pub fn new_pcm() -> Decoder<'a> {
        Decoder::PCM(NoOpDecoder {})
    }

    #[cfg(feature = "flac")]
    pub fn new_flac() -> Decoder<'a> {
        Decoder::Flac(FlacDecoder::new())
    }

    #[cfg(feature = "opus")]
    pub fn new_opus(
        config: &OpusMetadata,
        slot: &'a mut core::mem::MaybeUninit<opus_embedded::Decoder>,
    ) -> anyhow::Result<Decoder<'a>> {
        let c = match config.channel_count {
            1 => opus_embedded::Channels::Mono,
            2 => opus_embedded::Channels::Stereo,
            _ => panic!("unsupported channel configuration"),
        };
        let s = match config.sample_rate {
            48_000 => opus_embedded::SamplingRate::F48k,
            _ => panic!("only supports 48_000 sampling rate for opus"),
        };
        let dec = opus_embedded::Decoder::init_in(slot, s, c)
            .map_err(|e| anyhow::anyhow!("making opus decoder: {e}"))?;
        Ok(Decoder::Opus(OpusDecoder(dec)))
    }
}

#[enum_dispatch]
pub trait Decode<'a> {
    /// Returns total number of samples
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error>;
}

#[cfg(feature = "opus")]
impl<'a> Decode<'a> for OpusDecoder<'a> {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        // TODO: fec?
        Ok(self.0.decode(buf, out).context("decode")?.len())
    }
}

pub struct NoOpDecoder;

impl<'a> Decode<'a> for NoOpDecoder {
    fn decode_sample(&mut self, buf: &[u8], out: &mut [i16]) -> Result<usize, anyhow::Error> {
        // SAFETY: This is safe by design - a no-op decoder passes the data through as-is
        let (_, converted, _) = unsafe { buf.align_to::<i16>() };
        out[0..converted.len()].copy_from_slice(converted);

        Ok(converted.len())
    }
}

#[cfg(feature = "flac")]
pub struct FlacDecoder {
    dec_buf: Vec<i32>,
}
#[cfg(feature = "flac")]
impl FlacDecoder {
    pub fn new() -> FlacDecoder {
        FlacDecoder {
            dec_buf: Vec::with_capacity(2048),
        }
    }
}

#[cfg(feature = "flac")]
impl<'a> Decode<'a> for FlacDecoder {
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
