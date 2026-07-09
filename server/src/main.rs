mod config;
mod net;
mod pipeline;
mod sink;
mod source;
mod spotify;

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
    // keep the mDNS advertisement alive for the whole process
    let _mdns = net::advertise(&config.device_name, config.bind.port())?;
    {
        let registry = registry.clone();
        std::thread::spawn(move || net::accept_loop(listener, registry, clock));
    }

    match config.source {
        Source::Sine => {
            let pipeline =
                Pipeline::new(clock, registry.clone(), config.chunk_ms, config.opus_bitrate)?;
            source::run_sine(pipeline)?;
        }
        Source::Spotify => {
            // librespot needs an async runtime; the snapcast side stays threaded
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(spotify::run(&config, clock, registry.clone()))?;
        }
    }
    Ok(())
}
