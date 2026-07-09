use std::sync::mpsc::Sender;

use librespot::playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot::playback::convert::Converter;
use librespot::playback::decoder::AudioPacket;

use crate::pipeline::Pipeline;

/// Events that mark discontinuities in the source. Today `start`/`stop` re-anchor
/// the pipeline clock; a future buffer-flush-on-skip is one more variant here.
#[derive(Debug)]
pub enum SourceEvent {
    Started,
    Stopped,
    Paused,
    TrackChanged,
}

/// A librespot [`Sink`] that owns the [`Pipeline`]. librespot's player thread
/// calls `write` as fast as the sink accepts data, so the pipeline's pacing sleep
/// (inside `push`) is what throttles librespot's decode — exactly as a hardware
/// sink's blocking write would.
pub struct PipelineSink {
    pipeline: Pipeline,
    events: Sender<SourceEvent>,
    interleaved: Vec<f32>,
}

impl PipelineSink {
    pub fn new(pipeline: Pipeline, events: Sender<SourceEvent>) -> PipelineSink {
        PipelineSink {
            pipeline,
            events,
            interleaved: Vec::new(),
        }
    }
}

impl Sink for PipelineSink {
    fn start(&mut self) -> SinkResult<()> {
        self.pipeline.reanchor();
        let _ = self.events.send(SourceEvent::Started);
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        // pad the trailing partial opus frame with silence so it is emitted
        self.pipeline
            .flush_with_silence()
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        let _ = self.events.send(SourceEvent::Stopped);
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        self.interleaved.clear();
        self.interleaved.extend(samples.iter().map(|&s| s as f32));
        self.pipeline
            .push(&self.interleaved)
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        Ok(())
    }
}
