//! Hardware acceleration preference and GPU crash recovery.
//!
//! Speech recognition and summaries use the GPU when one is available. A GPU driver
//! fault aborts the process from inside ggml (a C++ exception Rust cannot catch), so
//! while whisper runs on the GPU a marker file exists in the app data directory. If
//! the app starts and finds the marker, the previous run died on the GPU: speech
//! recognition switches to the CPU and the user is told once. The summary sidecar is
//! a separate process, so its crashes are reported directly via `record_gpu_crash`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::command;

const SETTINGS_FILE: &str = "acceleration.json";
const TRANSCRIPTION_MARKER: &str = "gpu-transcription.inflight";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccelerationMode {
    /// GPU when one is available, otherwise CPU
    #[default]
    Auto,
    Cpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuEngine {
    Transcription,
    Summary,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccelerationSettings {
    pub mode: AccelerationMode,
    /// Switched to CPU after the GPU crashed during speech recognition
    pub transcription_gpu_crashed: bool,
    /// Switched to CPU after the GPU crashed while generating a summary
    pub summary_gpu_crashed: bool,
    /// A crash fallback happened that the user has not been told about yet
    pub crash_notice_pending: bool,
}

impl AccelerationSettings {
    pub fn use_gpu(&self, engine: GpuEngine) -> bool {
        let crashed = match engine {
            GpuEngine::Transcription => self.transcription_gpu_crashed,
            GpuEngine::Summary => self.summary_gpu_crashed,
        };
        self.mode == AccelerationMode::Auto && !crashed
    }
}

struct State {
    dir: PathBuf,
    settings: AccelerationSettings,
    transcription_in_flight: usize,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Load the saved preference and detect a GPU crash in the previous run
pub fn init(app_data_dir: &Path) {
    let mut settings: AccelerationSettings = std::fs::read_to_string(app_data_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    let marker = app_data_dir.join(TRANSCRIPTION_MARKER);
    if marker.exists() {
        log::warn!("The previous run stopped while speech recognition was using the GPU; switching it to the CPU");
        settings.transcription_gpu_crashed = true;
        settings.crash_notice_pending = true;
        let _ = std::fs::remove_file(&marker);
        save(app_data_dir, &settings);
    }

    log::info!("Hardware acceleration settings: {:?}", settings);
    *STATE.lock().unwrap() = Some(State {
        dir: app_data_dir.to_path_buf(),
        settings,
        transcription_in_flight: 0,
    });
}

fn save(dir: &Path, settings: &AccelerationSettings) {
    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(dir.join(SETTINGS_FILE), json) {
                log::error!("Failed to save hardware acceleration settings: {}", e);
            }
        }
        Err(e) => log::error!("Failed to serialize hardware acceleration settings: {}", e),
    }
}

/// Current settings (defaults when `init` has not run, e.g. in tests and benchmarks)
pub fn settings() -> AccelerationSettings {
    STATE.lock().unwrap().as_ref().map(|s| s.settings.clone()).unwrap_or_default()
}

/// Whether `engine` should use the GPU (if the build and machine have one)
pub fn use_gpu(engine: GpuEngine) -> bool {
    settings().use_gpu(engine)
}

/// Switch `engine` to the CPU after its GPU work crashed
pub fn record_gpu_crash(engine: GpuEngine) {
    let mut state = STATE.lock().unwrap();
    let Some(state) = state.as_mut() else { return };
    match engine {
        GpuEngine::Transcription => state.settings.transcription_gpu_crashed = true,
        GpuEngine::Summary => state.settings.summary_gpu_crashed = true,
    }
    state.settings.crash_notice_pending = true;
    save(&state.dir, &state.settings);
}

/// Marks GPU speech recognition as running until dropped
pub struct TranscriptionGpuScope(());

/// Call before GPU speech recognition work (model load, detection, decoding)
pub fn transcription_gpu_scope() -> TranscriptionGpuScope {
    let mut state = STATE.lock().unwrap();
    if let Some(state) = state.as_mut() {
        if state.transcription_in_flight == 0 {
            if let Err(e) = std::fs::write(state.dir.join(TRANSCRIPTION_MARKER), b"") {
                log::warn!("Failed to write GPU crash marker: {}", e);
            }
        }
        state.transcription_in_flight += 1;
    }
    TranscriptionGpuScope(())
}

impl Drop for TranscriptionGpuScope {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap();
        if let Some(state) = state.as_mut() {
            state.transcription_in_flight = state.transcription_in_flight.saturating_sub(1);
            if state.transcription_in_flight == 0 {
                let _ = std::fs::remove_file(state.dir.join(TRANSCRIPTION_MARKER));
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct GpuInfo {
    pub name: String,
    pub description: String,
    pub integrated: bool,
}

#[derive(Debug, Serialize)]
pub struct AccelerationStatus {
    #[serde(flatten)]
    pub settings: AccelerationSettings,
    /// GPU backend speech recognition was compiled with (Vulkan, Metal, CUDA...) or "Cpu"
    pub backend: &'static str,
    /// GPU speech recognition would use, if any
    pub gpu: Option<GpuInfo>,
}

#[command]
pub async fn get_acceleration_status() -> Result<AccelerationStatus, String> {
    use crate::whisper_engine::acceleration::{gpu_devices, preferred_gpu, WhisperCompiledBackend};

    let backend = WhisperCompiledBackend::current();
    let gpu = if backend == WhisperCompiledBackend::Cpu {
        None
    } else {
        tauri::async_runtime::spawn_blocking(|| {
            preferred_gpu(gpu_devices()).map(|device| GpuInfo {
                name: device.name.clone(),
                description: device.description.clone(),
                integrated: device.integrated,
            })
        })
        .await
        .map_err(|e| e.to_string())?
    };

    Ok(AccelerationStatus {
        settings: settings(),
        backend: backend.as_str(),
        gpu,
    })
}

/// Change the mode; choosing it again also clears crash fallbacks ("try the GPU again").
/// Takes effect the next time the models are loaded.
#[command]
pub async fn set_acceleration_mode(mode: AccelerationMode) -> Result<(), String> {
    {
        let mut state = STATE.lock().unwrap();
        let state = state.as_mut().ok_or("Hardware acceleration settings are not initialized")?;
        state.settings = AccelerationSettings {
            mode,
            ..AccelerationSettings::default()
        };
        save(&state.dir, &state.settings);
    }
    log::info!("Hardware acceleration mode set to {:?}", mode);

    // Reload speech recognition now, unless a recording is using it (then on next load)
    if !crate::audio::recording_commands::is_recording().await {
        let engine = crate::whisper_engine::commands::WHISPER_ENGINE.lock().unwrap().clone();
        if let Some(engine) = engine {
            engine.reload_current_model().await.map_err(|e| e.to_string())?;
        }
    }

    // Restart the summary sidecar so it picks up the new mode on its next request
    if let Err(e) = crate::summary::summary_engine::client::shutdown_sidecar_gracefully().await {
        log::warn!("Failed to restart the summary engine: {}", e);
    }
    Ok(())
}

#[command]
pub async fn acknowledge_acceleration_notice() -> Result<(), String> {
    let mut state = STATE.lock().unwrap();
    if let Some(state) = state.as_mut() {
        if state.settings.crash_notice_pending {
            state.settings.crash_notice_pending = false;
            save(&state.dir, &state.settings);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_mode_uses_gpu_until_an_engine_crashes() {
        let mut settings = AccelerationSettings::default();
        assert!(settings.use_gpu(GpuEngine::Transcription));
        assert!(settings.use_gpu(GpuEngine::Summary));

        settings.transcription_gpu_crashed = true;
        assert!(!settings.use_gpu(GpuEngine::Transcription));
        assert!(settings.use_gpu(GpuEngine::Summary));
    }

    #[test]
    fn cpu_mode_disables_gpu_for_every_engine() {
        let settings = AccelerationSettings { mode: AccelerationMode::Cpu, ..Default::default() };
        assert!(!settings.use_gpu(GpuEngine::Transcription));
        assert!(!settings.use_gpu(GpuEngine::Summary));
    }

    #[test]
    fn settings_file_tolerates_missing_fields() {
        let settings: AccelerationSettings = serde_json::from_str(r#"{"mode":"cpu"}"#).unwrap();
        assert_eq!(settings.mode, AccelerationMode::Cpu);
        assert!(!settings.transcription_gpu_crashed);
    }
}
