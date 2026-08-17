use std::ptr;

use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Media::Audio::Endpoints::IAudioEndpointVolume,
        Media::Audio::{IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender},
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize, STGM_READ,
        },
    },
    core::PCWSTR,
};

use crate::AppResult;

pub const VOLUME_STEP_PERCENT: i8 = 5;

pub struct AudioEndpoint {
    device_id: String,
    name: String,
}

impl AudioEndpoint {
    pub fn open_default() -> AppResult<(Self, u8)> {
        with_com(|| {
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole)? };
            let device_id = device_id(&device)?;
            let name = friendly_name(&device)?;
            let volume = endpoint_volume(&device)?;
            let volume_percent = read_volume_percent(&volume)?;

            Ok((Self { device_id, name }, volume_percent))
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn current_volume_percent(&self) -> AppResult<u8> {
        self.with_volume(read_volume_percent)
    }

    pub fn adjust_volume(&self, delta_percent: i8) -> AppResult<u8> {
        self.with_volume(|volume| {
            let current = unsafe { volume.GetMasterVolumeLevelScalar()? };
            let target = adjusted_scalar(current, delta_percent);
            unsafe { volume.SetMasterVolumeLevelScalar(target, ptr::null())? };
            read_volume_percent(volume)
        })
    }

    fn with_volume<T>(
        &self,
        operation: impl FnOnce(&IAudioEndpointVolume) -> AppResult<T>,
    ) -> AppResult<T> {
        self.with_device(|device| {
            let volume: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None)? };
            operation(&volume)
        })
    }

    fn with_device<T>(&self, operation: impl FnOnce(IMMDevice) -> AppResult<T>) -> AppResult<T> {
        with_com(|| {
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let wide_id: Vec<u16> = self
                .device_id
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let device = unsafe { enumerator.GetDevice(PCWSTR::from_raw(wide_id.as_ptr()))? };
            operation(device)
        })
    }
}

fn with_com<T>(operation: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let result = operation();
    unsafe { CoUninitialize() };
    result
}

fn device_id(device: &IMMDevice) -> AppResult<String> {
    let id = unsafe { device.GetId()? };
    let result = unsafe { id.to_string() };
    unsafe { CoTaskMemFree(Some(id.0 as _)) };
    Ok(result?)
}

fn friendly_name(device: &IMMDevice) -> AppResult<String> {
    let properties = unsafe { device.OpenPropertyStore(STGM_READ)? };
    let value = unsafe { properties.GetValue(&PKEY_Device_FriendlyName)? };
    let name = value.to_string();
    if name.is_empty() {
        Ok("Windows default render endpoint".to_owned())
    } else {
        Ok(name)
    }
}

fn endpoint_volume(device: &IMMDevice) -> AppResult<IAudioEndpointVolume> {
    Ok(unsafe { device.Activate(CLSCTX_ALL, None)? })
}

fn read_volume_percent(volume: &IAudioEndpointVolume) -> AppResult<u8> {
    let scalar = unsafe { volume.GetMasterVolumeLevelScalar()? };
    Ok(scalar_to_percent(scalar))
}

fn adjusted_scalar(current: f32, delta_percent: i8) -> f32 {
    (current + f32::from(delta_percent) / 100.0).clamp(0.0, 1.0)
}

fn scalar_to_percent(scalar: f32) -> u8 {
    (scalar.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_scalar_clamps_at_zero_and_one() {
        assert_eq!(adjusted_scalar(0.02, -VOLUME_STEP_PERCENT), 0.0);
        assert_eq!(adjusted_scalar(0.98, VOLUME_STEP_PERCENT), 1.0);
    }

    #[test]
    fn volume_scalar_converts_to_nearest_integer_percent() {
        assert_eq!(scalar_to_percent(0.424), 42);
        assert_eq!(scalar_to_percent(0.425), 43);
    }
}
