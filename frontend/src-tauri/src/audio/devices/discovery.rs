use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::error;

use super::configuration::{AudioDevice, DeviceType};
use super::platform;
use crate::audio::host_thread::on_audio_host_thread;

/// List all available audio devices on the system
pub async fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    on_audio_host_thread(|| {
        let host = cpal::default_host();

        // Platform-specific device enumeration
        let mut devices = {
            #[cfg(target_os = "windows")]
            {
                platform::configure_windows_audio(&host)?
            }

            #[cfg(target_os = "linux")]
            {
                platform::configure_linux_audio(&host)?
            }

            #[cfg(target_os = "macos")]
            {
                platform::configure_macos_audio(&host)?
            }
        };

        // Add any additional devices from the default host
        if let Ok(other_devices) = host.devices() {
            for device in other_devices {
                if let Ok(name) = device.name() {
                    if !devices.iter().any(|d| d.name == name) {
                        devices.push(AudioDevice::new(name, DeviceType::Output));
                    }
                }
            }
        }

        Ok(devices)
    })
}

/// Trigger audio permission request on platforms that require it
/// Returns Ok(true) if permission is granted, Ok(false) if denied, Err if something went wrong
pub fn trigger_audio_permission() -> Result<bool> {
    use log::info;

    // Device lookup goes through the audio host thread; the stream itself is
    // built here so the permission prompt keeps its current threading.
    let lookup = on_audio_host_thread(|| {
        let host = cpal::default_host();
        let device = host.default_input_device()?;
        let config = device.default_input_config().ok()?;
        Some((device, config))
    });

    let (device, config) = match lookup {
        Some(pair) => pair,
        None => {
            info!("[trigger_audio_permission] No usable default input device found - permission likely denied");
            return Ok(false);
        }
    };

    // Build and start an input stream to trigger the permission request
    let stream = match device.build_input_stream(
        &config.into(),
        |_data: &[f32], _: &cpal::InputCallbackInfo| {
            // Do nothing, we just want to trigger the permission request
        },
        |err| error!("Error in audio stream: {}", err),
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            info!("[trigger_audio_permission] Failed to build input stream: {} - permission likely denied", e);
            return Ok(false);
        }
    };

    // Start the stream to actually trigger the permission dialog
    if let Err(e) = stream.play() {
        info!("[trigger_audio_permission] Failed to play stream: {} - permission likely denied", e);
        return Ok(false);
    }

    // Sleep briefly to allow the permission dialog to appear and for stream to actually work
    std::thread::sleep(std::time::Duration::from_millis(500));

    // If we got here, permission was granted
    info!("[trigger_audio_permission] Stream played successfully - permission granted");

    // Stop the stream
    drop(stream);

    Ok(true)
}