//! Sound playback (rodio): the decoded PCM of a sound, from a position, with
//! the position read back for the playhead. The output device opens on the
//! first play.

use std::time::{Duration, Instant};

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink};
use tempest_assets::audio::Pcm;

#[derive(Default)]
pub struct Player {
    out: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    /// When it started and from which second, and how long it plays.
    started: Option<(Instant, f64, f64)>,
}

impl Player {
    /// Play `pcm` from `from` seconds (to `to`, else the end).
    pub fn play(&mut self, pcm: &Pcm, from: f64, to: Option<f64>) -> Result<(), String> {
        self.stop();
        if self.out.is_none() {
            self.out = Some(OutputStream::try_default().map_err(|e| format!("no audio output: {e}"))?);
        }
        let (_, handle) = self.out.as_ref().unwrap();
        let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
        let rate = pcm.sample_rate as f64;
        let n = pcm.frames();
        let a = ((from.max(0.0) * rate) as usize).min(n);
        let b = to.map(|t| ((t * rate) as usize).clamp(a, n)).unwrap_or(n);
        let ch = pcm.channels.len().max(1);
        let mut data = Vec::with_capacity((b - a) * ch);
        for f in a..b {
            for c in &pcm.channels {
                data.push(c[f]);
            }
        }
        sink.append(SamplesBuffer::new(ch as u16, pcm.sample_rate, data));
        sink.play();
        self.sink = Some(sink);
        self.started = Some((Instant::now(), from, (b - a) as f64 / rate));
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.started = None;
    }

    /// The second being played, while it plays.
    pub fn position(&mut self) -> Option<f64> {
        let (at, from, len) = self.started?;
        let t = at.elapsed().as_secs_f64();
        if t >= len || self.sink.as_ref().is_none_or(|s| s.empty()) {
            self.started = None;
            return None;
        }
        Some(from + t)
    }

    pub fn playing(&self) -> bool {
        self.started.is_some()
    }
}

/// How often the playhead moves.
pub const TICK: Duration = Duration::from_millis(33);
