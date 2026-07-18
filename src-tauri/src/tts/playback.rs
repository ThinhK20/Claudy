use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, Player};

/// Volume is stored as fixed-point milli-units (0..=1000) so it can live in an
/// atomic and be read lock-free from the playback thread.
const VOLUME_SCALE: f32 = 1000.0;

enum Cmd {
    Play { samples: Vec<f32>, sample_rate: u32 },
    Stop,
}

/// TTS audio playback. cpal streams are `!Send` on Windows, so the rodio
/// `Player` and its output stream live entirely on a dedicated thread; all
/// control crosses over via the command channel and shared atomics.
pub struct Playback {
    tx: Mutex<Option<Sender<Cmd>>>,
    playing: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            tx: Mutex::new(None),
            playing: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicU32::new(VOLUME_SCALE as u32)),
        }
    }
}

impl Playback {
    /// True while audio is queued or actively playing.
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Set playback volume (0.0..=1.0). Applied on the next thread tick.
    pub fn set_volume(&self, v: f32) {
        let milli = (v.clamp(0.0, 1.0) * VOLUME_SCALE).round() as u32;
        self.volume.store(milli, Ordering::Relaxed);
    }

    /// Stop and clear anything queued. Safe to call when idle.
    pub fn stop(&self) {
        if let Ok(guard) = self.tx.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(Cmd::Stop);
            }
        }
        self.playing.store(false, Ordering::Relaxed);
    }

    /// Queue audio for playback, lazily starting the playback thread (which
    /// opens the default output device). Errors if no device can be opened.
    pub fn enqueue(&self, samples: Vec<f32>, sample_rate: u32) -> Result<(), String> {
        self.ensure_thread()?;
        self.playing.store(true, Ordering::Relaxed);
        let guard = self.tx.lock().map_err(|_| "playback state poisoned")?;
        guard
            .as_ref()
            .ok_or("playback thread unavailable")?
            .send(Cmd::Play { samples, sample_rate })
            .map_err(|_| "playback thread stopped".to_string())
    }

    fn ensure_thread(&self) -> Result<(), String> {
        let mut guard = self.tx.lock().map_err(|_| "playback state poisoned")?;
        if guard.is_some() {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let playing = self.playing.clone();
        let volume = self.volume.clone();
        std::thread::spawn(move || run_thread(rx, ready_tx, playing, volume));
        match ready_rx.recv() {
            Ok(Ok(())) => {
                *guard = Some(tx);
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Playback thread failed to start".into()),
        }
    }
}

/// Owns the rodio player for its whole lifetime. Reports device-open success
/// through `ready` before entering the command loop.
fn run_thread(
    rx: Receiver<Cmd>,
    ready: Sender<Result<(), String>>,
    playing: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
) {
    let sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(format!("Could not open audio device: {e}")));
            return;
        }
    };
    let player = Player::connect_new(sink.mixer());
    if ready.send(Ok(())).is_err() {
        return;
    }

    let mono = NonZero::new(1u16).unwrap();
    let mut last_volume = u32::MAX;
    loop {
        let vol = volume.load(Ordering::Relaxed);
        if vol != last_volume {
            player.set_volume(vol as f32 / VOLUME_SCALE);
            last_volume = vol;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Cmd::Play { samples, sample_rate }) => {
                let rate = NonZero::new(sample_rate).unwrap_or(NonZero::new(24_000).unwrap());
                player.append(SamplesBuffer::new(mono, rate, samples));
                // `clear()` (used by Stop) leaves the player paused; resume or
                // every append after the first stop is silent forever.
                player.play();
                playing.store(true, Ordering::Relaxed);
            }
            Ok(Cmd::Stop) => {
                player.clear();
                playing.store(false, Ordering::Relaxed);
            }
            Err(RecvTimeoutError::Timeout) => {
                // Poll for drain so callers can transition out of "speaking".
                if playing.load(Ordering::Relaxed) && player.empty() {
                    playing.store(false, Ordering::Relaxed);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: rodio's `Player::clear()` pauses the sink, so a stop
    /// followed by new audio must explicitly resume playback or the player
    /// stays paused forever (silent, `is_playing()` stuck at true).
    #[test]
    fn enqueue_after_stop_still_plays_and_drains() {
        let playback = Playback::default();
        let samples = vec![0.0_f32; 4800]; // 0.2 s of silence @ 24 kHz
        if let Err(e) = playback.enqueue(samples.clone(), 24_000) {
            eprintln!("skipping: no audio device ({e})");
            return;
        }
        playback.stop();
        playback.enqueue(samples, 24_000).expect("second enqueue failed");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while playback.is_playing() {
            assert!(
                std::time::Instant::now() < deadline,
                "playback never drained after stop + enqueue: player stuck paused"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
