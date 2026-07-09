use std::sync::Arc;
use std::time::Duration;

use audiopus::coder::Encoder;
use audiopus::{Application, Bitrate, Channels, SampleRate};
use rubato::{FftFixedIn, Resampler};

use snapcast_client::proto::TimeVal;

use crate::net::{EncodedChunk, Registry, ServerClock};

const SOURCE_RATE: usize = 44_100;
const OUTPUT_RATE: usize = 48_000;
/// Resampler input batch. 882 = 20ms @ 44.1k and a multiple of 44100/gcd(44100,48000),
/// which the FFT resampler requires; decoupled from `chunk_ms` on purpose.
const RESAMPLE_IN_CHUNK: usize = 882;

fn opus<T>(r: audiopus::Result<T>) -> anyhow::Result<T> {
    r.map_err(|e| anyhow::anyhow!("opus: {e:?}"))
}

/// Turns interleaved 44.1k f32 audio into paced, opus-encoded snapcast chunks.
/// It runs inline on whichever thread feeds it (the sine source, or later
/// librespot's player thread); its pacing sleep is what throttles that source.
pub struct Pipeline {
    resampler: FftFixedIn<f32>,
    in_chunk: usize,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    frame: usize,
    encoder: Encoder,
    enc_in: Vec<f32>,
    enc_out: Vec<u8>,
    clock: ServerClock,
    registry: Arc<Registry>,
    chunk_ms: i64,
    t0_us: Option<i64>,
    chunks_since_anchor: i64,
}

impl Pipeline {
    pub fn new(
        clock: ServerClock,
        registry: Arc<Registry>,
        chunk_ms: u32,
        opus_bitrate: i32,
    ) -> anyhow::Result<Pipeline> {
        // opus only encodes 2.5/5/10/20/40/60ms frames; at 48k those are the only
        // chunk sizes it will accept, so reject anything else up front
        anyhow::ensure!(
            matches!(chunk_ms, 5 | 10 | 20 | 40 | 60),
            "chunk_ms must be one of 5, 10, 20, 40, 60 (opus frame sizes), got {chunk_ms}"
        );
        let frame = OUTPUT_RATE * chunk_ms as usize / 1000;
        let resampler =
            FftFixedIn::<f32>::new(SOURCE_RATE, OUTPUT_RATE, RESAMPLE_IN_CHUNK, 2, 2)?;
        let in_chunk = resampler.input_frames_next();

        let mut encoder = opus(Encoder::new(
            SampleRate::Hz48000,
            Channels::Stereo,
            Application::Audio,
        ))?;
        opus(encoder.set_bitrate(Bitrate::BitsPerSecond(opus_bitrate)))?;

        Ok(Pipeline {
            resampler,
            in_chunk,
            in_l: Vec::with_capacity(in_chunk * 2),
            in_r: Vec::with_capacity(in_chunk * 2),
            out_l: Vec::with_capacity(frame * 2),
            out_r: Vec::with_capacity(frame * 2),
            frame,
            encoder,
            enc_in: vec![0.0; frame * 2],
            enc_out: vec![0u8; 4096],
            clock,
            registry,
            chunk_ms: chunk_ms as i64,
            t0_us: None,
            chunks_since_anchor: 0,
        })
    }

    /// Pad the trailing partial opus frame with silence and emit it, so the tail
    /// of a track is not swallowed when the source stops.
    pub fn flush_with_silence(&mut self) -> anyhow::Result<()> {
        let rem = self.out_l.len() % self.frame;
        if rem != 0 {
            let pad = self.frame - rem;
            self.out_l.resize(self.out_l.len() + pad, 0.0);
            self.out_r.resize(self.out_r.len() + pad, 0.0);
        }
        while self.out_l.len() >= self.frame {
            self.emit_frame()?;
        }
        Ok(())
    }

    /// Restart the clock anchor and drop any partially accumulated audio.
    pub fn reanchor(&mut self) {
        self.t0_us = None;
        self.chunks_since_anchor = 0;
        self.in_l.clear();
        self.in_r.clear();
        self.out_l.clear();
        self.out_r.clear();
    }

    pub fn push(&mut self, interleaved: &[f32]) -> anyhow::Result<()> {
        for frame in interleaved.chunks_exact(2) {
            self.in_l.push(frame[0]);
            self.in_r.push(frame[1]);
        }

        while self.in_l.len() >= self.in_chunk {
            let l: Vec<f32> = self.in_l.drain(..self.in_chunk).collect();
            let r: Vec<f32> = self.in_r.drain(..self.in_chunk).collect();
            let out = self.resampler.process(&[l, r], None)?;
            self.out_l.extend_from_slice(&out[0]);
            self.out_r.extend_from_slice(&out[1]);
        }

        while self.out_l.len() >= self.frame {
            self.emit_frame()?;
        }
        Ok(())
    }

    fn emit_frame(&mut self) -> anyhow::Result<()> {
        for (i, (l, r)) in self
            .out_l
            .drain(..self.frame)
            .zip(self.out_r.drain(..self.frame))
            .enumerate()
        {
            self.enc_in[2 * i] = l;
            self.enc_in[2 * i + 1] = r;
        }
        let n = opus(self.encoder.encode_float(&self.enc_in, &mut self.enc_out))?;
        let data = self.enc_out[..n].to_vec();

        let timestamp = TimeVal::from_micros(self.pace());
        self.registry
            .broadcast_chunk(Arc::new(EncodedChunk { timestamp, data }));
        self.chunks_since_anchor += 1;
        Ok(())
    }

    /// Sleep until this chunk's presentation instant, returning that server-clock
    /// timestamp (microseconds). Re-anchors if we have fallen more than one chunk
    /// behind, so a stalled source does not emit a burst of past-dated chunks.
    fn pace(&mut self) -> i64 {
        let chunk_us = self.chunk_ms * 1000;
        let t0 = *self.t0_us.get_or_insert_with(|| self.clock.now_us());
        let mut target = t0 + self.chunks_since_anchor * chunk_us;

        let now = self.clock.now_us();
        if now - target > chunk_us {
            self.t0_us = Some(now);
            self.chunks_since_anchor = 0;
            target = now;
        }

        let now = self.clock.now_us();
        if target > now {
            std::thread::sleep(Duration::from_micros((target - now) as u64));
        }
        target
    }
}
