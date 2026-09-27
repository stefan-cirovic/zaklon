//! What the hub knows about the computer it runs on: drives (for copying
//! packs to and from USB) and a short hardware summary (for the Household
//! screen and, later, for recommending AI models).

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Drive {
    /// Root folder, like "E:\".
    pub path: String,
    pub label: String,
    /// "removable" (USB sticks, SD cards) or "fixed" (internal and most USB disks).
    pub kind: &'static str,
    /// File system name, like "NTFS", "exFAT" or "FAT32".
    pub file_system: String,
    pub free: u64,
    pub total: u64,
    /// The drive Windows runs from.
    pub system: bool,
}

impl Drive {
    /// FAT32 cannot hold files of 4 GiB or more.
    pub fn max_file_size(&self) -> Option<u64> {
        (self.file_system.eq_ignore_ascii_case("FAT32") || self.file_system.eq_ignore_ascii_case("FAT")).then_some(u32::MAX as u64)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Hardware {
    pub cpu: String,
    pub cores: usize,
    pub ram_total: u64,
    pub ram_free: u64,
    pub os: String,
}

pub fn hardware() -> Hardware {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let (ram_total, ram_free) = memory();
    Hardware { cpu: cpu_name(), cores, ram_total, ram_free, os: os_name() }
}

/// The drive that holds `path`, if it is one of ours.
pub fn drive_of(path: &Path) -> Option<Drive> {
    let p = drive_key(path);
    drives().into_iter().filter(|d| p.starts_with(&drive_key(Path::new(&d.path)))).max_by_key(|d| d.path.len())
}

/// A path in the form drive roots are compared in: upper case, backslashes,
/// ending in one ("E:" and "e:/x" become "E:\" and "E:\X\").
fn drive_key(path: &Path) -> String {
    let mut p = path.to_string_lossy().to_uppercase().replace('/', "\\");
    if !p.ends_with('\\') {
        p.push('\\');
    }
    p
}

#[cfg(windows)]
pub fn drives() -> Vec<Drive> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW};
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    let mut buf = [0u16; 512];
    // SAFETY: the buffer and its length are passed together.
    let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
    let mut out = Vec::new();
    for root in buf[..n.min(buf.len())].split(|c| *c == 0).filter(|s| !s.is_empty()) {
        let mut wide: Vec<u16> = root.to_vec();
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated root path.
        let kind = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
            DRIVE_REMOVABLE => "removable",
            DRIVE_FIXED => "fixed",
            _ => continue, // network, CD/DVD, RAM disks
        };
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        // SAFETY: every out-buffer is passed with its length; the rest may be null.
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        };
        if ok == 0 {
            continue; // card reader without a card, locked drive...
        }
        let text = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())]);
        let path = String::from_utf16_lossy(root);
        let (free, total) = (fs4::available_space(&path).unwrap_or(0), fs4::total_space(&path).unwrap_or(0));
        let system = path.to_uppercase().starts_with(&system_drive);
        out.push(Drive { path, label: text(&label), kind, file_system: text(&fs), free, total, system });
    }
    out
}

#[cfg(not(windows))]
pub fn drives() -> Vec<Drive> {
    Vec::new()
}

#[cfg(windows)]
fn memory() -> (u64, u64) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is plain data; zeroed is a valid starting value.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: the struct is sized and owned by us.
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return (0, 0);
    }
    (m.ullTotalPhys, m.ullAvailPhys)
}

#[cfg(not(windows))]
fn memory() -> (u64, u64) {
    let info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let field = |name: &str| {
        info.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0)
    };
    (field("MemTotal:"), field("MemAvailable:"))
}

#[cfg(windows)]
fn registry_string(key: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (k, v) = (wide(key), wide(value));
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: strings are NUL-terminated; the buffer is passed with its size in bytes.
    let rc = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
    };
    if rc != 0 {
        return None;
    }
    let chars = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars]);
    Some(s.trim_end_matches('\0').trim().to_string())
}

#[cfg(windows)]
fn cpu_name() -> String {
    registry_string(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").unwrap_or_default()
}

#[cfg(windows)]
fn os_name() -> String {
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let mut name = registry_string(key, "ProductName").unwrap_or_else(|| "Windows".into());
    // Windows 11 still reports "Windows 10" here; the build number tells them apart.
    if let Some(build) = registry_string(key, "CurrentBuildNumber").and_then(|b| b.parse::<u32>().ok()) {
        if build >= 22000 {
            name = name.replace("Windows 10", "Windows 11");
        }
    }
    match registry_string(key, "DisplayVersion") {
        Some(v) if !v.is_empty() => format!("{name} {v}"),
        _ => name,
    }
}

#[cfg(not(windows))]
fn cpu_name() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn os_name() -> String {
    std::env::consts::OS.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_is_read() {
        let h = hardware();
        assert!(h.cores >= 1);
        assert!(h.ram_total > 0);
        #[cfg(windows)]
        {
            assert!(!h.cpu.is_empty());
            assert!(h.os.starts_with("Windows"), "{}", h.os);
            let d = drives();
            assert!(d.iter().any(|d| d.system), "the system drive is listed: {d:?}");
        }
    }

    #[test]
    fn a_typed_drive_letter_matches_its_root() {
        let root = drive_key(Path::new("E:\\"));
        assert_eq!(root, "E:\\");
        for typed in ["E:", "e:", "E:\\", "e:/packs", "E:\\Packs\\"] {
            assert!(drive_key(Path::new(typed)).starts_with(&root), "{typed}");
        }
        assert!(!drive_key(Path::new("F:")).starts_with(&root));
    }

    #[test]
    fn fat32_limits_file_size() {
        let d = Drive { path: "E:\\".into(), label: String::new(), kind: "removable", file_system: "FAT32".into(), free: 0, total: 0, system: false };
        assert_eq!(d.max_file_size(), Some(4_294_967_295));
        let n = Drive { file_system: "exFAT".into(), ..d };
        assert_eq!(n.max_file_size(), None);
    }
}
