#[cfg(feature = "alsa")]
pub mod alsa;
#[cfg(feature = "alsa")]
pub(crate) use alsa::Alsa;

#[cfg(feature = "pulse")]
pub(crate) use pulse::Pulse;
#[cfg(feature = "pulse")]
pub mod pulse;

pub mod file;
pub(crate) use file::File;

use enum_dispatch::enum_dispatch;

#[enum_dispatch]
pub(crate) trait Player {
    fn play(&self) -> anyhow::Result<()>;
    fn write(&mut self, buf: &[i16]) -> anyhow::Result<()>;
}

#[enum_dispatch(Player)]
pub enum Players {
    #[cfg(feature = "alsa")]
    Alsa,
    #[cfg(feature = "pulse")]
    Pulse,
    File,
}
