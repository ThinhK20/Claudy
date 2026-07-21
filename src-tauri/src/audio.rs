use serde::Serialize;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const TARGET_SAMPLE_RATE: u32 = 16_000;
const LEVEL_EMIT_INTERVAL_MS: u64 = 50;
const SETUP_TIMEOUT_SECS: u64 = 5;

pub fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

pub fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((samples.len() as f64) / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = samples[idx];
            let b = samples[(idx + 1).min(samples.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

pub struct CaptureHandle {
    stop_tx: mpsc::Sender<()>,
    join: JoinHandle<Result<Vec<f32>, String>>,
}

#[derive(Default)]
pub struct AudioState(pub Mutex<Option<CaptureHandle>>);

#[derive(Clone, Serialize)]
struct MicLevel {
    level: f32,
}

struct StreamParts {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<f32>>,
    channels: u16,
    sample_rate: u32,
}

// Device matching is name-based (user-facing selection), so keep DeviceTrait::name
// even though rodio's cpal re-export marks it deprecated.
#[allow(deprecated)]
fn build_stream(device_name: &str) -> Result<StreamParts, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_input_device()
            .ok_or("No microphone found. Connect a microphone and try again.")?
    } else {
        host.input_devices()
            .map_err(|e| format!("Could not list microphones: {e}"))?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
            .ok_or_else(|| format!("Microphone '{device_name}' not found"))?
    };

    let supported = device
        .default_input_config()
        .map_err(|e| format!("Microphone is unavailable or busy: {e}"))?;
    let channels = supported.channels();
    let sample_rate = supported.sample_rate();
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();

    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let level = Arc::new(Mutex::new(0f32));

    let err_cb = |e: cpal::StreamError| eprintln!("audio stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            let buf = buffer.clone();
            let lvl = level.clone();
            device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| ingest(data, &buf, &lvl),
                    err_cb,
                    None,
                )
                .map_err(|e| format!("Could not open microphone: {e}"))?
        }
        cpal::SampleFormat::I16 => {
            let buf = buffer.clone();
            let lvl = level.clone();
            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let floats: Vec<f32> =
                            data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                        ingest(&floats, &buf, &lvl);
                    },
                    err_cb,
                    None,
                )
                .map_err(|e| format!("Could not open microphone: {e}"))?
        }
        other => return Err(format!("Unsupported microphone sample format: {other:?}")),
    };

    Ok(StreamParts { stream, buffer, level, channels, sample_rate })
}

fn ingest(data: &[f32], buffer: &Arc<Mutex<Vec<f32>>>, level: &Arc<Mutex<f32>>) {
    if let Ok(mut buf) = buffer.lock() {
        buf.extend_from_slice(data);
    }
    if !data.is_empty() {
        let rms = (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt();
        if let Ok(mut l) = level.lock() {
            *l = rms;
        }
    }
}

/// Spawns the capture thread. Returns once the stream is confirmed running.
pub fn start(app: AppHandle, device_name: String) -> Result<CaptureHandle, String> {
    use cpal::traits::StreamTrait;

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    let join = std::thread::spawn(move || -> Result<Vec<f32>, String> {
        let parts = match build_stream(&device_name) {
            Ok(p) => p,
            Err(e) => {
                let _ = ready_tx.send(Err(e.clone()));
                return Err(e);
            }
        };
        if let Err(e) = parts.stream.play() {
            let e = format!("Could not start microphone: {e}");
            let _ = ready_tx.send(Err(e.clone()));
            return Err(e);
        }
        let _ = ready_tx.send(Ok(()));

        loop {
            match stop_rx.recv_timeout(Duration::from_millis(LEVEL_EMIT_INTERVAL_MS)) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    let level = parts.level.lock().map(|l| *l).unwrap_or(0.0);
                    let _ = app.emit("mic-level", MicLevel { level });
                }
            }
        }

        drop(parts.stream); // stops the callback before we read the buffer
        let raw = parts
            .buffer
            .lock()
            .map(|b| b.clone())
            .map_err(|_| "audio buffer poisoned".to_string())?;
        let mono = downmix_to_mono(&raw, parts.channels);
        Ok(resample_linear(&mono, parts.sample_rate, TARGET_SAMPLE_RATE))
    });

    match ready_rx.recv_timeout(Duration::from_secs(SETUP_TIMEOUT_SECS)) {
        Ok(Ok(())) => Ok(CaptureHandle { stop_tx, join }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => Err("Microphone setup timed out".into()),
    }
}

/// Stops the capture thread and returns 16 kHz mono samples.
pub fn stop(state: &AudioState) -> Result<Vec<f32>, String> {
    let handle = state
        .0
        .lock()
        .map_err(|_| "audio state poisoned")?
        .take()
        .ok_or("Not recording")?;
    let _ = handle.stop_tx.send(());
    handle
        .join
        .join()
        .map_err(|_| "Capture thread panicked".to_string())?
}

#[tauri::command]
#[allow(deprecated)] // DeviceTrait::name: devices are identified by name for selection
pub fn list_audio_devices() -> Result<Vec<String>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("Could not list microphones: {e}"))?;
    Ok(devices.filter_map(|d| d.name().ok()).collect())
}

#[tauri::command]
pub fn start_capture(
    app: AppHandle,
    state: tauri::State<AudioState>,
    device: String,
) -> Result<(), String> {
    let mut slot = state.0.lock().map_err(|_| "audio state poisoned")?;
    if slot.is_some() {
        return Err("Already recording".into());
    }
    *slot = Some(start(app, device)?);
    Ok(())
}

#[tauri::command]
pub fn stop_capture(state: tauri::State<AudioState>) -> Result<(), String> {
    stop(&state).map(|_| ()) // mic test: samples are discarded, never stored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo_frames() {
        let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        let mono = [0.1, 0.2, 0.3];
        assert_eq!(downmix_to_mono(&mono, 1), mono.to_vec());
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let s = [0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&s, 16_000, 16_000), s.to_vec());
    }

    #[test]
    fn resample_48k_to_16k_yields_one_third_length() {
        let s: Vec<f32> = (0..48_000).map(|i| (i % 100) as f32 / 100.0).collect();
        let out = resample_linear(&s, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn resample_preserves_constant_signal() {
        let s = vec![0.5f32; 4410];
        let out = resample_linear(&s, 44_100, 16_000);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-6));
        assert_eq!(out.len(), 1600);
    }
}
