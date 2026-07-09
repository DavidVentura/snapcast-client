mod config;
mod net;
mod pipeline;
mod source;

use std::net::TcpListener;
use std::sync::Arc;

use clap::Parser;

use config::{Config, Source};
use net::{Registry, ServerClock};
use pipeline::Pipeline;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let config = Config::parse();
    log::info!(
        "starting: bind={} buffer_ms={} chunk_ms={} opus_bitrate={} device={:?} cache={:?} source={:?}",
        config.bind,
        config.buffer_ms,
        config.chunk_ms,
        config.opus_bitrate,
        config.device_name,
        config.cache_dir,
        config.source,
    );

    let clock = ServerClock::new();
    let registry = Arc::new(Registry::new(config.buffer_ms, 100));

    let listener = TcpListener::bind(config.bind)?;
    log::info!("listening on {}", config.bind);
    {
        let registry = registry.clone();
        std::thread::spawn(move || net::accept_loop(listener, registry, clock));
    }

    let pipeline = Pipeline::new(clock, registry.clone(), config.chunk_ms, config.opus_bitrate)?;
    match config.source {
        Source::Sine => source::run_sine(pipeline)?,
    }
    Ok(())
}
