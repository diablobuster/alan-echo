//! ALAN Echo — Audio device enumeration.
//! Recording will be handled by the frontend via Web Audio API + Tauri commands.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub index: usize,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn list_input_devices() -> Result<Vec<DeviceInfo>, String> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let devices: Vec<DeviceInfo> = host
        .input_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?
        .enumerate()
        .filter_map(|(i, device)| {
            let name = device.name().ok()?;
            let config = device.default_input_config().ok()?;
            Some(DeviceInfo {
                is_default: name == default_name,
                name,
                index: i,
                sample_rate: config.sample_rate().0,
                channels: config.channels(),
            })
        })
        .collect();

    Ok(devices)
}
