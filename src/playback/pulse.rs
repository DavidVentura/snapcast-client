use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

pub struct PulsePlayer {
    pulse: Simple,
}

impl PulsePlayer {
    pub fn new() -> PulsePlayer {
        let spec = Spec {
            format: Format::S16NE,
            channels: 2,
            rate: 48000,
        };
        let pulse = Simple::new(
            None,                // Use the default server
            "FooApp",            // Our application’s name
            Direction::Playback, // We want a playback stream
            None,                // Use the default device
            "Music",             // Description of our stream
            &spec,               // Our sample format
            None,                // Use default channel map
            None,                // Use default buffering attributes
        )
        .unwrap();

        PulsePlayer { pulse }
    }

    pub fn play(&self) {}
    pub fn write(&self, buf: &[i16]) {
        let (_, converted, _) = unsafe { buf.align_to::<u8>() };
        self.pulse.write(converted).unwrap();
    }
}
