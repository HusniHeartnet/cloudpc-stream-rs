//! Penyetelan Windows selama stream berjalan, seperti `streaming_will_start`/`streaming_will_stop` Sunshine
//! (`src/platform/windows/misc.cpp`) dan mode latensi rendah moonlight-common-c. Semua dikembalikan saat `StreamingMode`
//! di-drop.
//!
//! - Timer Windows 0,5 ms (`NtSetTimerResolution`, cadangan `timeBeginPeriod(1)`): `sleep` dan pengatur waktu lebih tepat.
//! - Prioritas proses tinggi.
//! - Wi-Fi: mode streaming media + pemindaian latar dimatikan pada antarmuka yang tersambung (pemindaian latar
//!   menyebabkan paket hilang dan jitter tiap beberapa detik).
//! - Host saja: penjadwalan MMCSS untuk DWM, dan Mouse Keys bila tidak ada mouse fisik (tanpa itu Windows menyembunyikan
//!   kursor; temuan uji sesi #84).

use std::ffi::c_void;

use windows::core::{s, w, GUID};
use windows::Win32::Foundation::{FreeLibrary, HANDLE, HMODULE};
use windows::Win32::Graphics::Dwm::DwmEnableMMCSS;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::NetworkManagement::WiFi::{wlan_interface_state_connected, wlan_intf_opcode_background_scan_enabled, wlan_intf_opcode_media_streaming_mode, WLAN_INTERFACE_INFO_LIST};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, HIGH_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS};
use windows::Win32::UI::Accessibility::MOUSEKEYS;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SystemParametersInfoW, MKF_AVAILABLE, MKF_MOUSEKEYSON, SM_MOUSEPRESENT, SPI_GETMOUSEKEYS, SPI_SETMOUSEKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Host: MMCSS untuk DWM dan Mouse Keys bila tidak ada mouse.
    pub host: bool,
}

/// Apa saja yang benar-benar diubah, untuk dicatat di log.
#[derive(Debug, Default, Clone)]
pub struct Applied {
    pub timer: &'static str,
    pub high_priority: bool,
    pub wlan_interfaces: u32,
    pub dwm_mmcss: bool,
    pub mouse_keys: bool,
}

type NtSetTimerResolution = unsafe extern "system" fn(u32, u8, *mut u32) -> i32;
type WlanOpenHandle = unsafe extern "system" fn(u32, *const c_void, *mut u32, *mut HANDLE) -> u32;
type WlanCloseHandle = unsafe extern "system" fn(HANDLE, *const c_void) -> u32;
type WlanEnumInterfaces = unsafe extern "system" fn(HANDLE, *const c_void, *mut *mut WLAN_INTERFACE_INFO_LIST) -> u32;
type WlanSetInterface = unsafe extern "system" fn(HANDLE, *const GUID, i32, u32, *const c_void, *const c_void) -> u32;
type WlanFreeMemory = unsafe extern "system" fn(*const c_void);

pub struct StreamingMode {
    pub applied: Applied,
    nt_timer: Option<NtSetTimerResolution>,
    time_period: bool,
    previous_mouse_keys: Option<MOUSEKEYS>,
    /// wlanapi.dll dimuat dinamis (tidak ada di Windows Server); menutup handle mengembalikan setelan Wi-Fi.
    wlan: Option<(HMODULE, HANDLE, WlanCloseHandle)>,
}

impl StreamingMode {
    pub fn enter(options: Options) -> StreamingMode {
        let mut mode = StreamingMode { applied: Applied::default(), nt_timer: None, time_period: false, previous_mouse_keys: None, wlan: None };
        unsafe {
            // Timer 0,5 ms (satuan 100 ns).
            let ntdll = GetModuleHandleW(w!("ntdll.dll")).ok();
            let set: Option<NtSetTimerResolution> = ntdll.and_then(|m| GetProcAddress(m, s!("NtSetTimerResolution"))).map(|f| std::mem::transmute(f));
            let mut actual = 0u32;
            if let Some(set) = set.filter(|set| set(5000, 1, &mut actual) >= 0) {
                mode.nt_timer = Some(set);
                mode.applied.timer = "0,5 ms";
            } else if timeBeginPeriod(1) == 0 {
                mode.time_period = true;
                mode.applied.timer = "1 ms";
            }
            mode.applied.high_priority = SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS).is_ok();
            mode.enter_wlan();
            if options.host {
                mode.applied.dwm_mmcss = DwmEnableMMCSS(true).is_ok();
                if GetSystemMetrics(SM_MOUSEPRESENT) == 0 {
                    let mut previous = MOUSEKEYS { cbSize: std::mem::size_of::<MOUSEKEYS>() as u32, ..Default::default() };
                    if SystemParametersInfoW(SPI_GETMOUSEKEYS, previous.cbSize, Some(&mut previous as *mut _ as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() {
                        let mut on = MOUSEKEYS { cbSize: previous.cbSize, dwFlags: MKF_MOUSEKEYSON | MKF_AVAILABLE, iMaxSpeed: 10, iTimeToMaxSpeed: 1000, ..Default::default() };
                        if SystemParametersInfoW(SPI_SETMOUSEKEYS, on.cbSize, Some(&mut on as *mut _ as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() {
                            mode.previous_mouse_keys = Some(previous);
                            mode.applied.mouse_keys = true;
                        }
                    }
                }
            }
        }
        mode
    }

    unsafe fn enter_wlan(&mut self) {
        let Ok(module) = LoadLibraryExW(w!("wlanapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else { return };
        let get = |name: windows::core::PCSTR| GetProcAddress(module, name);
        let (Some(open), Some(close), Some(enumerate), Some(set), Some(free)) =
            (get(s!("WlanOpenHandle")), get(s!("WlanCloseHandle")), get(s!("WlanEnumInterfaces")), get(s!("WlanSetInterface")), get(s!("WlanFreeMemory")))
        else {
            let _ = FreeLibrary(module);
            return;
        };
        let open: WlanOpenHandle = std::mem::transmute(open);
        let close: WlanCloseHandle = std::mem::transmute(close);
        let enumerate: WlanEnumInterfaces = std::mem::transmute(enumerate);
        let set: WlanSetInterface = std::mem::transmute(set);
        let free: WlanFreeMemory = std::mem::transmute(free);
        let (mut version, mut handle) = (0u32, HANDLE::default());
        if open(2, std::ptr::null(), &mut version, &mut handle) != 0 {
            let _ = FreeLibrary(module);
            return;
        }
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if enumerate(handle, std::ptr::null(), &mut list) == 0 && !list.is_null() {
            let count = (*list).dwNumberOfItems as usize;
            let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), count);
            for item in items.iter().filter(|i| i.isState == wlan_interface_state_connected) {
                let (on, off) = (1i32, 0i32);
                let streaming = set(handle, &item.InterfaceGuid, wlan_intf_opcode_media_streaming_mode.0, 4, &on as *const i32 as *const c_void, std::ptr::null());
                let _ = set(handle, &item.InterfaceGuid, wlan_intf_opcode_background_scan_enabled.0, 4, &off as *const i32 as *const c_void, std::ptr::null());
                if streaming == 0 {
                    self.applied.wlan_interfaces += 1;
                }
            }
            free(list as *const c_void);
        }
        self.wlan = Some((module, handle, close));
    }
}

impl Drop for StreamingMode {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous) = self.previous_mouse_keys.take() {
                let mut previous = previous;
                let _ = SystemParametersInfoW(SPI_SETMOUSEKEYS, previous.cbSize, Some(&mut previous as *mut _ as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            }
            if self.applied.dwm_mmcss {
                let _ = DwmEnableMMCSS(false);
            }
            // Menutup handle WLAN mengembalikan mode streaming dan pemindaian latar ke semula.
            if let Some((module, handle, close)) = self.wlan.take() {
                close(handle, std::ptr::null());
                let _ = FreeLibrary(module);
            }
            let _ = SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS);
            if let Some(set) = self.nt_timer.take() {
                let mut actual = 0u32;
                set(5000, 0, &mut actual);
            }
            if self.time_period {
                let _ = timeEndPeriod(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_and_leaving_restores_normal_priority() {
        let mode = StreamingMode::enter(Options { host: false });
        assert!(!mode.applied.timer.is_empty(), "timer dinaikkan: {:?}", mode.applied);
        assert!(!mode.applied.mouse_keys, "Mouse Keys hanya untuk host");
        drop(mode);
        let class = unsafe { windows::Win32::System::Threading::GetPriorityClass(GetCurrentProcess()) };
        assert_eq!(class, NORMAL_PRIORITY_CLASS.0, "prioritas kembali normal");
    }
}
