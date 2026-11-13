//! Voice checks on the call's own microphone stream.
//!
//! The candidate's integrity check, with voice checks allowed, listens to the
//! samples the call is already sending, through [`VoiceTap`], instead of
//! opening the microphone a second time. While listening is off (the
//! default) the tap only passes samples through. While it is on, each chunk
//! goes through [`VoiceActivityDetector`] and is dropped; the only thing that
//! leaves is a `tutoring:voice-onset` event with a time and a speaker label.
//! Listening stops while the microphone is muted and when the call ends.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use live::media::av::{AudioFormat, AudioSource};

use super::events::{emit, CallEvents};
use crate::sentinel::voice_activity::VoiceActivityDetector;

/// Playback level at which the far end counts as audible on this device.
const REMOTE_AUDIBLE: f32 = 0.05;
/// Most channels a microphone format is downmixed from.
const MAX_CHANNELS: usize = 8;

pub struct VoiceListener {
    listening: AtomicBool,
    muted: AtomicBool,
    remote_audible: AtomicBool,
    /// The detector and the sample rate it was made for.
    detector: Mutex<Option<(u32, VoiceActivityDetector)>>,
    events: Arc<dyn CallEvents>,
}

impl VoiceListener {
    pub fn new(events: Arc<dyn CallEvents>) -> Self {
        Self {
            listening: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            remote_audible: AtomicBool::new(false),
            detector: Mutex::new(None),
            events,
        }
    }

    /// Start or stop listening. Stopping forgets the voices heard so far.
    pub fn set_listening(&self, on: bool) {
        self.listening.store(on, Ordering::SeqCst);
        if !on {
            *self.detector.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
    }

    pub fn is_listening(&self) -> bool {
        self.listening.load(Ordering::SeqCst)
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
    }

    /// The far end's playback level, as the call meters it.
    pub fn set_remote_level(&self, level: f32) {
        self.remote_audible
            .store(level >= REMOTE_AUDIBLE, Ordering::SeqCst);
    }

    /// Hear one chunk of interleaved samples.
    pub fn hear(&self, format: AudioFormat, samples: &[f32], now_ms: u64) {
        if !self.is_listening() || self.muted.load(Ordering::SeqCst) {
            return;
        }
        let channels = (format.channel_count as usize).clamp(1, MAX_CHANNELS);
        let mono: Vec<f32> = samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        let remote = self.remote_audible.load(Ordering::SeqCst);
        let onsets = {
            let mut slot = self.detector.lock().unwrap_or_else(|p| p.into_inner());
            if slot
                .as_ref()
                .is_none_or(|(rate, _)| *rate != format.sample_rate)
            {
                *slot = Some((
                    format.sample_rate,
                    VoiceActivityDetector::new(format.sample_rate),
                ));
            }
            match slot.as_mut() {
                Some((_, detector)) => detector.push(&mono, now_ms, remote),
                None => Vec::new(),
            }
        };
        for onset in onsets {
            emit(self.events.as_ref(), "tutoring:voice-onset", onset);
        }
    }
}

/// A microphone source that lets the [`VoiceListener`] hear what it hands
/// the encoder.
pub struct VoiceTap {
    inner: Box<dyn AudioSource>,
    listener: Arc<VoiceListener>,
}

impl VoiceTap {
    pub fn new(source: impl AudioSource, listener: Arc<VoiceListener>) -> Self {
        Self {
            inner: Box::new(source),
            listener,
        }
    }
}

impl AudioSource for VoiceTap {
    fn cloned_boxed(&self) -> Box<dyn AudioSource> {
        Box::new(Self {
            inner: self.inner.cloned_boxed(),
            listener: self.listener.clone(),
        })
    }

    fn format(&self) -> AudioFormat {
        self.inner.format()
    }

    fn pop_samples(&mut self, buf: &mut [f32]) -> anyhow::Result<Option<usize>> {
        let popped = self.inner.pop_samples(buf)?;
        if let Some(n) = popped {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.listener
                .hear(self.inner.format(), &buf[..n.min(buf.len())], now_ms);
        }
        Ok(popped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tutoring::events::RecordedEvents;

    const RATE: u32 = 48_000;

    /// One second of a voiced sound at 130 Hz after half a second of quiet,
    /// in stereo.
    fn speech_after_quiet() -> Vec<f32> {
        let quiet = (0..RATE / 2).flat_map(|_| [0.0, 0.0]);
        let voice = (0..RATE).flat_map(|i| {
            let t = f64::from(i) / f64::from(RATE);
            let s: f64 = (1..=10)
                .map(|k| {
                    let k = f64::from(k);
                    (2.0 * std::f64::consts::PI * 130.0 * k * t).sin() / k
                })
                .sum();
            let s = s as f32 * 0.08;
            [s, s]
        });
        quiet.chain(voice).collect()
    }

    fn feed(listener: &VoiceListener, audio: &[f32]) {
        let format = AudioFormat::stereo_48k();
        for (i, chunk) in audio.chunks(960).enumerate() {
            listener.hear(format, chunk, 1_700_000_000_000 + i as u64 * 10);
        }
    }

    #[test]
    fn nothing_is_heard_until_listening_is_on_and_nothing_while_muted() {
        let events = Arc::new(RecordedEvents::default());
        let listener = VoiceListener::new(events.clone());
        let audio = speech_after_quiet();
        feed(&listener, &audio);
        assert!(events.payloads("tutoring:voice-onset").is_empty());

        listener.set_listening(true);
        listener.set_muted(true);
        feed(&listener, &audio);
        assert!(events.payloads("tutoring:voice-onset").is_empty());

        listener.set_muted(false);
        feed(&listener, &audio);
        let onsets = events.payloads("tutoring:voice-onset");
        assert_eq!(onsets.len(), 1, "{onsets:?}");
        assert_eq!(onsets[0]["speaker"], "primary");
        assert!(onsets[0]["at_ms"].as_u64().is_some());
        // The payload is a time and a label, nothing heard.
        assert_eq!(onsets[0].as_object().unwrap().len(), 2);
    }

    #[test]
    fn the_far_end_playing_keeps_the_microphone_from_being_heard() {
        let events = Arc::new(RecordedEvents::default());
        let listener = VoiceListener::new(events.clone());
        listener.set_listening(true);
        listener.set_remote_level(0.4);
        feed(&listener, &speech_after_quiet());
        assert!(events.payloads("tutoring:voice-onset").is_empty());
    }
}
