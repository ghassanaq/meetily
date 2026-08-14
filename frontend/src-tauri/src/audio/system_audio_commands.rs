use tauri::{command, AppHandle, Emitter, State};
use crate::audio::{
    start_system_audio_capture, list_system_audio_devices, check_system_audio_permissions,
    SystemAudioDetector, SystemAudioEvent, new_system_audio_callback
};
use std::sync::{Arc, Mutex};
use anyhow::Result;

// Global state for system audio detector
type SystemAudioDetectorState = Arc<Mutex<Option<SystemAudioDetector>>>;

/// Start system audio capture (for capturing system output audio)
#[command]
pub async fn start_system_audio_capture_command() -> Result<String, String> {
    match start_system_audio_capture().await {
        Ok(_stream) => {
            // TODO: Store the stream in global state if needed for management
            Ok("System audio capture started successfully".to_string())
        }
        Err(e) => Err(format!("Failed to start system audio capture: {}", e))
    }
}

/// List available system audio devices
#[command]
pub async fn list_system_audio_devices_command() -> Result<Vec<String>, String> {
    list_system_audio_devices()
        .map_err(|e| format!("Failed to list system audio devices: {}", e))
}

/// Check if the app has permission to access system audio
#[command]
pub async fn check_system_audio_permissions_command() -> bool {
    check_system_audio_permissions()
}

/// Start monitoring system audio usage by other applications
#[command]
pub async fn start_system_audio_monitoring(
    app_handle: AppHandle,
    detector_state: State<'_, SystemAudioDetectorState>
) -> Result<(), String> {
    let mut detector_guard = detector_state.lock()
        .map_err(|e| format!("Failed to acquire detector lock: {}", e))?;

    if detector_guard.is_some() {
        return Err("System audio monitoring is already active".to_string());
    }

    let mut detector = SystemAudioDetector::new();

    // Create callback that emits events to the frontend
    let callback = new_system_audio_callback(move |event| {
        match event {
            SystemAudioEvent::SystemAudioStarted(apps) => {
                tracing::info!("System audio started by apps: {:?}", apps);
                let _ = app_handle.emit("system-audio-started", apps);
            }
            SystemAudioEvent::SystemAudioStopped => {
                let _ = app_handle.emit("system-audio-stopped", ());
                tracing::info!("System audio stopped");
            }
        }
    });

    detector.start(callback);
    *detector_guard = Some(detector);

    Ok(())
}

/// Stop monitoring system audio usage
#[command]
pub async fn stop_system_audio_monitoring(
    detector_state: State<'_, SystemAudioDetectorState>
) -> Result<(), String> {
    let mut detector_guard = detector_state.lock()
        .map_err(|e| format!("Failed to acquire detector lock: {}", e))?;

    if let Some(mut detector) = detector_guard.take() {
        detector.stop();
        Ok(())
    } else {
        Err("System audio monitoring is not active".to_string())
    }
}

/// Get the current status of system audio monitoring
#[command]
pub async fn get_system_audio_monitoring_status(
    detector_state: State<'_, SystemAudioDetectorState>
) -> Result<bool, String> {
    let detector_guard = detector_state.lock()
        .map_err(|e| format!("Failed to acquire detector lock: {}", e))?;

    Ok(detector_guard.is_some())
}

/// Initialize the system audio detector state in Tauri app
pub fn init_system_audio_state() -> SystemAudioDetectorState {
    Arc::new(Mutex::new(None))
}

// Event payload types for frontend
#[derive(serde::Serialize, Clone)]
pub struct SystemAudioStartedPayload {
    pub apps: Vec<String>,
}

#[derive(serde::Serialize, Clone)]
pub struct SystemAudioStoppedPayload;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_system_audio_devices() {
        let devices = list_system_audio_devices_command().await;
        match devices {
            Ok(device_list) => {
                println!("System audio devices: {:?}", device_list);
            }
            Err(e) => {
                println!("Error listing devices: {}", e);
                // This might fail on CI or systems without audio
            }
        }
    }

    #[tokio::test]
    async fn test_check_permissions() {
        let has_permission = check_system_audio_permissions_command().await;
        println!("Has system audio permissions: {}", has_permission);
        // This is mainly a smoke test to ensure it doesn't crash
    }
    /// Regression test for the `cargo test --workspace` abort with exit code
    /// 0xC0000005 (STATUS_ACCESS_VIOLATION) on Windows.
    ///
    /// The two commands above used to be enough to kill the whole test binary:
    /// `check_system_audio_permissions_command` builds a cpal device iterator
    /// and drops it undrained, and libtest runs every test on its own
    /// short-lived thread. When that thread exited, its thread-local COM guard
    /// ran `CoUninitialize`, COM unloaded the audio in-proc server, and cpal's
    /// process-global cached `IMMDeviceEnumerator` was left dangling - so
    /// `list_system_audio_devices_command` faulted reading its vtable.
    /// See `crate::audio::host_thread` and https://github.com/RustAudio/cpal/issues/1302.
    ///
    /// This test forces that sequence into a single test so it does not depend
    /// on test ordering. A regression aborts the process rather than failing an
    /// assertion - an access violation is not catchable in-process.
    ///
    /// Hardware-independent: it asserts nothing about which or how many devices
    /// exist, so it is valid on a machine with no usable audio endpoints (an RDP
    /// session without audio redirection, a VM, or a disabled output device).
    #[test]
    fn probe_then_enumerate_across_thread_exits_does_not_crash() {
        // Thread 1: probe permissions, then exit (runs CoUninitialize).
        std::thread::spawn(|| {
            let granted = check_system_audio_permissions();
            println!("permission probe: {}", granted);
        })
        .join()
        .expect("permission probe thread panicked");

        // Thread 2: enumerate against the now-orphaned cached enumerator.
        let devices = std::thread::spawn(list_system_audio_devices)
            .join()
            .expect("enumeration thread panicked");

        match devices {
            Ok(names) => println!("enumerated {} system audio devices", names.len()),
            // An error is fine - a hard crash is not.
            Err(e) => println!("enumeration returned an error (acceptable): {}", e),
        }
    }
}
