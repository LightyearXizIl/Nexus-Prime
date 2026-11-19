//! Metadata-only WASAPI inventory. Never probe formats to check device presence.
#[cfg(target_os = "windows")]
pub fn active_names(capture: bool) -> Result<Vec<String>, String> {
    use windows::Win32::Media::Audio::{
        eCapture, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
        STGM_READ,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        // An existing STA is usable too. Balance only our successful init call.
        if initialized.is_err() && initialized.0 != 0x80010106u32 as i32 {
            return Err(initialized.to_string());
        }
        struct Apartment(bool);
        impl Drop for Apartment {
            fn drop(&mut self) {
                if self.0 {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
        }
        let _apartment = Apartment(initialized.is_ok());
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let endpoints = enumerator
            .EnumAudioEndpoints(
                if capture { eCapture } else { eRender },
                DEVICE_STATE_ACTIVE,
            )
            .map_err(|e| e.to_string())?;
        let friendly_name = PROPERTYKEY {
            fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
            pid: 14,
        };
        let mut names = Vec::new();
        for index in 0..endpoints.GetCount().map_err(|e| e.to_string())? {
            let endpoint = endpoints.Item(index).map_err(|e| e.to_string())?;
            let store = endpoint
                .OpenPropertyStore(STGM_READ)
                .map_err(|e| e.to_string())?;
            names.push(
                store
                    .GetValue(&friendly_name)
                    .map_err(|e| e.to_string())?
                    .to_string(),
            );
        }
        Ok(names)
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    #[test]
    fn inventory_initializes_com_on_a_fresh_worker() {
        std::thread::spawn(|| {
            super::active_names(false).expect("render inventory on fresh worker");
            super::active_names(true).expect("capture inventory on fresh worker");
        })
        .join()
        .unwrap();
    }
}
