use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};

pub struct AlsaPlayer {
    pcm: PCM,
}

impl AlsaPlayer {
    pub fn new() -> AlsaPlayer {
        // Open default playback device
        let pcm = PCM::new("default", Direction::Playback, false).unwrap();

        // Set hardware parameters: 48000 Hz / Stereo / 16 bit
        {
            let hwp = HwParams::any(&pcm).unwrap();
            hwp.set_channels(2).unwrap();
            hwp.set_rate(48000, ValueOr::Nearest).unwrap();
            hwp.set_format(Format::s16()).unwrap();
            hwp.set_access(Access::RWInterleaved).unwrap();
            pcm.hw_params(&hwp).unwrap();
        }

        {
            // Make sure we don't start the stream too early
            let hwp = pcm.hw_params_current().unwrap();
            let swp = pcm.sw_params_current().unwrap();
            swp.set_start_threshold(hwp.get_buffer_size().unwrap())
                .unwrap();
            pcm.sw_params(&swp).unwrap();
        }

        AlsaPlayer { pcm }
    }
    pub fn play(&self) {
        if self.pcm.state() != State::Running {
            self.pcm.start().unwrap();
        }
    }
    pub fn write(&self, buf: &[i16]) {
        let io = self.pcm.io_i16().unwrap();
        io.writei(buf).unwrap();
    }
}
