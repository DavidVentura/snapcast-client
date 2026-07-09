use std::f32::consts::PI;

use crate::pipeline::Pipeline;

const FREQ: f32 = 440.0;
const AMPLITUDE: f32 = 0.2;
const BLOCK_FRAMES: usize = 882;

/// Generate an endless 440Hz sine at 44.1k and feed it to the pipeline. The
/// pipeline's pacing sleep provides the real-time cadence, so this loop is not
/// otherwise throttled.
pub fn run_sine(mut pipeline: Pipeline) -> anyhow::Result<()> {
    let mut phase = 0.0f32;
    let step = FREQ / SOURCE_RATE;
    let mut buf = vec![0.0f32; BLOCK_FRAMES * 2];
    loop {
        for frame in buf.chunks_exact_mut(2) {
            let s = (phase * 2.0 * PI).sin() * AMPLITUDE;
            frame[0] = s;
            frame[1] = s;
            phase += step;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
        pipeline.push(&buf)?;
    }
}

const SOURCE_RATE: f32 = 44_100.0;
