use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};

use super::Player;

pub struct AlsaPlayer {
    pcm: PCM,
}

impl AlsaPlayer {
    pub fn new() -> anyhow::Result<AlsaPlayer> {
        // Open default playback device
        let pcm = PCM::new("default", Direction::Playback, false)?;

        // Set hardware parameters: 48000 Hz / Stereo / 16 bit
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(2)?;
            hwp.set_rate(48000, ValueOr::Nearest)?;
            hwp.set_format(Format::s16())?;
            hwp.set_access(Access::RWInterleaved)?;
            pcm.hw_params(&hwp)?;
        }

        {
            // Make sure we don't start the stream too early
            let hwp = pcm.hw_params_current()?;
            let swp = pcm.sw_params_current()?;
            swp.set_start_threshold(hwp.get_buffer_size()?)?;
            pcm.sw_params(&swp)?;
        }

        Ok(AlsaPlayer { pcm })
    }
}
impl Player for AlsaPlayer {
    fn play(&self) -> anyhow::Result<()> {
        if self.pcm.state() != State::Running {
            self.pcm.start()?;
        }
        Ok(())
    }
    fn write(&self, buf: &[i16]) -> anyhow::Result<()> {
        let io = self.pcm.io_i16()?;
        io.writei(buf)?;
        Ok(())
    }
}
