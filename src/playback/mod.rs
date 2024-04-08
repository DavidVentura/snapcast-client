#[cfg(feature = "alsa")]
pub mod alsa;
#[cfg(feature = "alsa")]
pub use alsa::Alsa;

#[cfg(feature = "pulse")]
pub use pulse::Pulse;
#[cfg(feature = "pulse")]
pub mod pulse;

pub mod file;
pub use file::File;

pub mod tcp;
pub use tcp::Tcp;

use enum_dispatch::enum_dispatch;

#[enum_dispatch]
pub trait Player {
    fn play(&self) -> anyhow::Result<()>;
    fn write(&mut self, buf: &[i16]) -> anyhow::Result<()>;
    fn latency_ms(&self) -> anyhow::Result<u16>;
    fn set_volume(&self, val: u8) -> anyhow::Result<()>;
}

#[enum_dispatch(Player)]
pub enum Players {
    #[cfg(feature = "alsa")]
    Alsa,
    #[cfg(feature = "pulse")]
    Pulse,
    File,
    Tcp,
}
