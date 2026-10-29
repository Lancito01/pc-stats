// CPU and GPU load from Windows' performance counters, plus memory, uptime, idle time and
// the focused app. Counters are added by their English names, so they work on any Windows language.

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows_sys::Win32::System::SystemInformation::{
    GetTickCount, GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::util::wide;

const PDH_MORE_DATA: u32 = 0x8000_07D2;
// Lets "% Processor Utility" go past 100 when the CPU boosts; it's capped after averaging
const PDH_FMT_NOCAP100: u32 = 0x0000_8000;

/// Samples CPU and GPU every few seconds; `take` hands back the averages since the last call.
pub struct Load {
    query: PDH_HQUERY,
    cpu: PDH_HCOUNTER,
    gpu: PDH_HCOUNTER,
    cpu_total: f64,
    cpu_samples: u32,
    gpu_total: f64,
    gpu_samples: u32,
    last: (Option<u8>, Option<u8>),
}

impl Load {
    pub fn new() -> Load {
        let mut load = Load {
            query: std::ptr::null_mut(),
            cpu: std::ptr::null_mut(),
            gpu: std::ptr::null_mut(),
            cpu_total: 0.0,
            cpu_samples: 0,
            gpu_total: 0.0,
            gpu_samples: 0,
            last: (None, None),
        };
        unsafe {
            if PdhOpenQueryW(std::ptr::null(), 0, &mut load.query) != 0 {
                return load;
            }
            // "% Processor Utility" is what Task Manager shows; older systems only have "% Processor Time"
            if PdhAddEnglishCounterW(load.query, wide(r"\Processor Information(_Total)\% Processor Utility").as_ptr(), 0, &mut load.cpu) != 0 {
                PdhAddEnglishCounterW(load.query, wide(r"\Processor(_Total)\% Processor Time").as_ptr(), 0, &mut load.cpu);
            }
            // One instance per process per GPU engine; adding up the 3D ones gives the GPU's load
            PdhAddEnglishCounterW(load.query, wide(r"\GPU Engine(*engtype_3D)\Utilization Percentage").as_ptr(), 0, &mut load.gpu);
            // Rate counters need a first reading to compare against
            PdhCollectQueryData(load.query);
        }
        load
    }

    pub fn sample(&mut self) {
        if self.query.is_null() || unsafe { PdhCollectQueryData(self.query) } != 0 {
            return;
        }
        if let Some(cpu) = self.value(self.cpu) {
            self.cpu_total += cpu.min(100.0);
            self.cpu_samples += 1;
        }
        if let Some(gpu) = self.sum(self.gpu) {
            self.gpu_total += gpu.min(100.0);
            self.gpu_samples += 1;
        }
    }

    /// Average CPU and GPU percentages since the last call. With no new sample in between
    /// (updates and samples can line up that way), the previous reading is repeated.
    pub fn take(&mut self) -> (Option<u8>, Option<u8>) {
        let avg = |total: f64, n: u32| (n > 0).then(|| (total / n as f64).round().clamp(0.0, 100.0) as u8);
        let cpu = avg(self.cpu_total, self.cpu_samples).or(self.last.0);
        let gpu = avg(self.gpu_total, self.gpu_samples).or(self.last.1);
        (self.cpu_total, self.cpu_samples, self.gpu_total, self.gpu_samples) = (0.0, 0, 0.0, 0);
        self.last = (cpu, gpu);
        self.last
    }

    fn value(&self, counter: PDH_HCOUNTER) -> Option<f64> {
        if counter.is_null() {
            return None;
        }
        let mut value: PDH_FMT_COUNTERVALUE = unsafe { std::mem::zeroed() };
        let status = unsafe {
            PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100, std::ptr::null_mut(), &mut value)
        };
        (status == 0 && value.CStatus <= 1).then_some(unsafe { value.Anonymous.doubleValue })
    }

    fn sum(&self, counter: PDH_HCOUNTER) -> Option<f64> {
        if counter.is_null() {
            return None;
        }
        let (mut size, mut count) = (0u32, 0u32);
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut())
        };
        if status != PDH_MORE_DATA || size == 0 {
            return None;
        }
        // The items are followed by their instance names in the same buffer
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, items) } != 0 {
            return None;
        }
        let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
        Some(
            items
                .iter()
                .filter(|item| item.FmtValue.CStatus <= 1)
                .map(|item| unsafe { item.FmtValue.Anonymous.doubleValue })
                .sum(),
        )
    }
}

/// Used and total physical memory in GB.
pub fn memory() -> Option<(f64, f64)> {
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    let gb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    Some((gb(status.ullTotalPhys - status.ullAvailPhys), gb(status.ullTotalPhys)))
}

pub fn uptime_seconds() -> u64 {
    (unsafe { GetTickCount64() }) / 1000
}

/// Seconds since the last keyboard or mouse input anywhere on the PC.
pub fn idle_seconds() -> u64 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0;
    }
    // Both are 32-bit tick counts, so subtract with wraparound
    (unsafe { GetTickCount() }.wrapping_sub(info.dwTime) / 1000) as u64
}

/// The file name of the focused app's exe, e.g. "code.exe". Window titles are never read.
pub fn focused_exe() -> Option<String> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut len) != 0;
        CloseHandle(process);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&path[..len as usize]);
        path.rsplit('\\').next().map(|name| name.to_string())
    }
}
