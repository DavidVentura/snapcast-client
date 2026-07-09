use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

use futures::StreamExt;
use librespot::connect::{ConnectConfig, Spirc};
use librespot::core::authentication::Credentials;
use librespot::core::cache::Cache;
use librespot::core::config::{DeviceType, SessionConfig};
use librespot::core::session::Session;
use librespot::discovery::Discovery;
use librespot::playback::audio_backend::Sink;
use librespot::playback::config::PlayerConfig;
use librespot::playback::mixer::{self, MixerConfig, Mixer, NoOpVolume};
use librespot::playback::player::{Player, PlayerEvent};

use crate::config::Config;
use crate::net::{Registry, ServerClock};
use crate::pipeline::Pipeline;
use crate::sink::{PipelineSink, SourceEvent};

/// Advertise on Spotify Connect and, whenever a phone picks us, run a librespot
/// session whose audio flows through the pipeline and whose volume changes are
/// re-broadcast as snapcast settings (so they hit the DAC immediately).
pub async fn run(
    config: &Config,
    clock: ServerClock,
    registry: Arc<Registry>,
) -> anyhow::Result<()> {
    let session_config = SessionConfig::default();
    let player_config = PlayerConfig::default();

    let cache = Cache::new(
        Some(config.cache_dir.as_path()),
        Some(config.cache_dir.as_path()),
        None,
        None,
    )?;

    let mixer_fn =
        mixer::find(None).ok_or_else(|| anyhow::anyhow!("no software mixer available"))?;
    let mixer = mixer_fn(MixerConfig::default())?;

    let mut discovery = Discovery::builder(
        session_config.device_id.clone(),
        session_config.client_id.clone(),
    )
    .name(config.device_name.clone())
    .device_type(DeviceType::Speaker)
    .launch()?;

    log::info!("advertising '{}' on Spotify Connect", config.device_name);

    // one channel funnels every source discontinuity; today it only logs, but a
    // buffer-flush-on-skip would just consume the same stream
    let (source_tx, source_rx) = channel::<SourceEvent>();
    std::thread::spawn(move || {
        while let Ok(ev) = source_rx.recv() {
            log::debug!("source event: {ev:?}");
        }
    });

    loop {
        // reconnect silently with cached credentials; first run waits for a phone
        let credentials = match cache.credentials() {
            Some(c) => c,
            None => match discovery.next().await {
                Some(c) => c,
                None => {
                    log::warn!("discovery stream ended");
                    return Ok(());
                }
            },
        };

        if let Err(e) = run_session(
            config,
            clock,
            &registry,
            &session_config,
            &player_config,
            &cache,
            &mixer,
            credentials,
            &source_tx,
        )
        .await
        {
            log::warn!("spotify session ended: {e}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    config: &Config,
    clock: ServerClock,
    registry: &Arc<Registry>,
    session_config: &SessionConfig,
    player_config: &PlayerConfig,
    cache: &Cache,
    mixer: &Arc<dyn Mixer>,
    credentials: Credentials,
    source_tx: &Sender<SourceEvent>,
) -> anyhow::Result<()> {
    let session = Session::new(session_config.clone(), Some(cache.clone()));

    let clk = clock;
    let reg = registry.clone();
    let chunk_ms = config.chunk_ms;
    let bitrate = config.opus_bitrate;
    let sink_tx = source_tx.clone();
    // NoOpVolume keeps PCM full-scale; volume is applied downstream at each DAC
    let player = Player::new(player_config.clone(), session.clone(), Box::new(NoOpVolume), move || {
        let pipeline = Pipeline::new(clk, reg, chunk_ms, bitrate).expect("failed to build pipeline");
        Box::new(PipelineSink::new(pipeline, sink_tx)) as Box<dyn Sink>
    });

    let vol_registry = registry.clone();
    let event_tx = source_tx.clone();
    let mut events = player.get_player_event_channel();
    let event_task = tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            match ev {
                PlayerEvent::VolumeChanged { volume } => {
                    let percent = ((volume as u32 * 100 + 32767) / 65535) as u8;
                    log::info!("spotify volume -> {percent}%");
                    vol_registry.broadcast_settings(percent);
                }
                PlayerEvent::Paused { .. } => {
                    let _ = event_tx.send(SourceEvent::Paused);
                }
                PlayerEvent::TrackChanged { .. } => {
                    let _ = event_tx.send(SourceEvent::TrackChanged);
                }
                _ => {}
            }
        }
    });

    let connect_config = ConnectConfig {
        name: config.device_name.clone(),
        device_type: DeviceType::Speaker,
        ..Default::default()
    };

    let (spirc, spirc_task) = Spirc::new(
        connect_config,
        session.clone(),
        credentials,
        player.clone(),
        mixer.clone(),
    )
    .await?;

    spirc.activate()?;
    spirc_task.await;
    event_task.abort();
    Ok(())
}
