#[cfg(feature = "alsa")]
pub mod alsa;
#[cfg(feature = "alsa")]
pub use alsa::AlsaPlayer;

#[cfg(feature = "pulse")]
pub use pulse::PulsePlayer;
#[cfg(feature = "pulse")]
pub mod pulse;

use enum_dispatch::enum_dispatch;
use std::io::Write;

#[enum_dispatch]
pub(crate) trait Player {
    fn play(&self) -> anyhow::Result<()>;
    fn write(&mut self, buf: &[i16]) -> anyhow::Result<()>;
}

#[enum_dispatch(Player)]
pub enum Players {
    #[cfg(feature = "alsa")]
    AlsaPlayer,
    #[cfg(feature = "pulse")]
    PulsePlayer,
    FilePlayer,
}

pub(crate) struct FilePlayer {
    f: std::fs::File,
}
impl Player for FilePlayer {
    fn play(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn write(&mut self, buf: &[i16]) -> anyhow::Result<()> {
        // SAFETY: it's always safe to align i16 to u8
        let (_, converted, _) = unsafe { buf.align_to::<u8>() };
        self.f.write(converted)?;
        Ok(())
    }
}

impl FilePlayer {
    pub fn new(p: &std::path::Path) -> anyhow::Result<FilePlayer> {
        Ok(FilePlayer {
            f: std::fs::File::create(p)?,
        })
    }
}
