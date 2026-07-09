use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Source {
    /// Embedded librespot; appears as a Spotify Connect device.
    Spotify,
    /// A synthetic sine tone, useful for exercising the pipeline without Spotify.
    Sine,
}

#[derive(Parser, Debug)]
#[command(about = "A snapcast server with an embedded audio pipeline")]
pub struct Config {
    /// Address to listen for snapclients on.
    #[arg(long, default_value = "0.0.0.0:1704")]
    pub bind: SocketAddr,

    /// How far ahead of playback the server timestamps chunks, in milliseconds.
    #[arg(long, default_value_t = 2000)]
    pub buffer_ms: u32,

    /// Duration of a single opus frame / wire chunk, in milliseconds.
    #[arg(long, default_value_t = 20)]
    pub chunk_ms: u32,

    /// Opus target bitrate, in bits per second.
    #[arg(long, default_value_t = 96_000)]
    pub opus_bitrate: i32,

    /// Name advertised to Spotify Connect.
    #[arg(long, default_value = "snapcast-rs")]
    pub device_name: String,

    /// Directory for the librespot credential/audio cache.
    #[arg(long, default_value = "/tmp/snapcast-cache")]
    pub cache_dir: PathBuf,

    /// Audio source feeding the pipeline.
    #[arg(long, value_enum, default_value_t = Source::Spotify)]
    pub source: Source,
}
