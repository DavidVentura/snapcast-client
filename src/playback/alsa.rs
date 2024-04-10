use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};

use super::Player;

pub struct Alsa {
    pcm: PCM,
    buf_time_ms: u16,
}

impl Alsa {
    pub fn new(rate: usize) -> anyhow::Result<Alsa> {
        // Open default playback device
        let pcm = PCM::new("default", Direction::Playback, false)?;

        // going below this gets no audio on my device
        let req_bufsize = 300;

        let buf_time_us = {
            // Set hardware parameters: 48000 Hz / Stereo / 16 bit
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(2)?;
            hwp.set_rate(rate as u32, ValueOr::Nearest)?;
            hwp.set_format(Format::s16())?;
            hwp.set_access(Access::RWInterleaved)?;
            hwp.set_buffer_size(req_bufsize)?;
            hwp.set_period_size(req_bufsize / 4, alsa::ValueOr::Nearest)?;
            pcm.hw_params(&hwp)?;
            // Make sure we don't start the stream too early
            // https://github.com/diwic/alsa-rs/blob/4d9735152b1a37554fb4aed74f3cb164d93bcf03/synth-example/src/main.rs#L85
            // Copied from synth example
            let (bufsize, periodsize) = (hwp.get_buffer_size()?, hwp.get_period_size()?);
            let hwp = pcm.hw_params_current()?;
            let swp = pcm.sw_params_current()?;
            swp.set_start_threshold(bufsize - periodsize)?;
            swp.set_avail_min(periodsize)?;
            pcm.sw_params(&swp)?;

            // us as defined in https://www.alsa-project.org/alsa-doc/alsa-lib/group___p_c_m___h_w___params.html#gaa18c9999c27632f6c47e163b6af17fa9
            (hwp.get_buffer_time_min()? + hwp.get_buffer_time_max()?) / 2
        };

        Ok(Alsa {
            pcm,
            buf_time_ms: (buf_time_us / 1000) as u16,
        })
    }
}

impl Player for Alsa {
    fn play(&mut self) -> anyhow::Result<()> {
        if self.pcm.state() != State::Running {
            self.pcm.start()?;
        }
        Ok(())
    }
    fn write(&mut self, buf: &mut [i16]) -> anyhow::Result<()> {
        let io = self.pcm.io_i16()?;
        io.writei(buf)?;
        Ok(())
    }
    fn latency_ms(&self) -> anyhow::Result<u16> {
        Ok(self.buf_time_ms)
    }
    fn set_volume(&mut self, val: u8) -> anyhow::Result<()> {
        Ok(())
    }
}
