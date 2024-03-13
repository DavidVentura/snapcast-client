pub mod alsa;
pub mod pulse;
pub use alsa::AlsaPlayer;
pub use pulse::PulsePlayer;

use enum_dispatch::enum_dispatch;

#[enum_dispatch]
pub(crate) trait Player {
    fn play(&self);
    fn write(&self, buf: &[i16]);
}

#[enum_dispatch(Player)]
pub enum Players {
    AlsaPlayer,
    PulsePlayer,
}
