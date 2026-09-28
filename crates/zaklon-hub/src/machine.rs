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

/// The computer's memory in bytes: all of it, and what is available now
/// (free, or holding only copies of files that can be dropped at once).
/// Cheap; (0, 0) when it cannot be read.
pub fn ram() -> (u64, u64) {
    memory()
}

/// The processor's cores, as the AI engine cares about them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cores {
    pub physical: usize,
    /// Threads the cores run together (two a core with simultaneous multithreading).
    pub logical: usize,
    /// Every core is of one kind and runs as many threads as the others: no
    /// mix of performance and efficiency cores.
    pub uniform: bool,
}

impl Cores {
    /// Threads for reading a prompt, when using every thread of the processor
    /// beats the engine's own choice (one a core): on alike cores that each
    /// run two threads. Measured on a Ryzen 5 1600 (6 cores, 12 threads) with
    /// the 9B model: 14 to 15 tokens a second with 6 threads, 17 to 20 with
    /// 12. Mixed performance and efficiency cores are left to the engine,
    /// where the slow cores would hold the fast ones back.
    pub fn prompt_threads(&self) -> Option<usize> {
        (self.uniform && self.physical >= 2 && self.logical == 2 * self.physical).then_some(self.logical)
    }
}

/// The processor's cores, read once.
pub fn cores() -> Option<Cores> {
    static CORES: std::sync::OnceLock<Option<Cores>> = std::sync::OnceLock::new();
    *CORES.get_or_init(read_cores)
}

#[cfg(all(windows, target_pointer_width = "64"))]
fn read_cores() -> Option<Cores> {
    use windows_sys::Win32::System::SystemInformation::{GetLogicalProcessorInformationEx, RelationProcessorCore};
    let mut len: u32 = 0;
    // SAFETY: with no buffer the call only says how many bytes it needs.
    unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut len) };
    if len == 0 {
        return None;
    }
    // u64s, so the records are aligned as Windows writes them.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: the buffer holds at least `len` bytes.
    if unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, buf.as_mut_ptr().cast(), &mut len) } == 0 {
        return None;
    }
    // SAFETY: `len` bytes of the buffer were written, and it has at least that many.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), (len as usize).min(buf.len() * 8)) };
    cores_from_records(bytes)
}

#[cfg(not(all(windows, target_pointer_width = "64")))]
fn read_cores() -> Option<Cores> {
    None
}

/// Cores from the records `GetLogicalProcessorInformationEx` writes for
/// `RelationProcessorCore`, one a core, in their 64-bit layout: the record's
/// kind at byte 0 and its size at 4, the core's efficiency class at 9, how
/// many processor groups it spans at 30 and the first group's mask of
/// threads at 32.
#[cfg_attr(not(all(windows, target_pointer_width = "64")), allow(dead_code))]
fn cores_from_records(bytes: &[u8]) -> Option<Cores> {
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let mut classes: Vec<u8> = Vec::new();
    let mut threads: Vec<u32> = Vec::new();
    let mut off = 0;
    while let (Some(kind), Some(size)) = (u32_at(off), u32_at(off + 4)) {
        let size = size as usize;
        if size < 40 || off + size > bytes.len() {
            break;
        }
        if kind == 0 {
            let mask = u64::from_le_bytes(bytes[off + 32..off + 40].try_into().ok()?);
            classes.push(bytes[off + 9]);
            threads.push(mask.count_ones());
        }
        off += size;
    }
    if threads.is_empty() {
        return None;
    }
    let uniform = classes.iter().all(|c| *c == classes[0]) && threads.iter().all(|t| *t == threads[0]);
    Some(Cores { physical: threads.len(), logical: threads.iter().map(|t| *t as usize).sum(), uniform })
}

/// The drive that holds `path`, if it is one of ours.
pub fn drive_of(path: &Path) -> Option<Drive> {
    let p = drive_key(path);
    drives().into_iter().filter(|d| p.starts_with(&drive_key(Path::new(&d.path)))).max_by_key(|d| d.path.len())
}

/// The root of the drive `path` is on, like "D:\" (on Windows), or "/" for
/// other systems; "" when it cannot be told. Cheap: nothing is read from disk.
pub fn drive_root(path: &Path) -> String {
    use std::path::{Component, Prefix};
    let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf()) };
    match abs.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(l) | Prefix::VerbatimDisk(l) => format!("{}:\\", (l as char).to_ascii_uppercase()),
            _ => p.as_os_str().to_string_lossy().into_owned(),
        },
        Some(Component::RootDir) => "/".into(),
        _ => String::new(),
    }
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
            let c = cores().expect("the cores are read on Windows");
            assert!(c.physical >= 1 && c.logical >= c.physical, "{c:?}");
            assert!(!h.cpu.is_empty());
            assert!(h.os.starts_with("Windows"), "{}", h.os);
            let d = drives();
            assert!(d.iter().any(|d| d.system), "the system drive is listed: {d:?}");
        }
    }

    /// A core's record as Windows writes it (64-bit layout, one processor group).
    fn core_record(class: u8, threads: u32) -> Vec<u8> {
        let mut r = vec![0u8; 48];
        r[4..8].copy_from_slice(&48u32.to_le_bytes());
        r[9] = class;
        r[30..32].copy_from_slice(&1u16.to_le_bytes());
        r[32..40].copy_from_slice(&((1u64 << threads) - 1).to_le_bytes());
        r
    }

    fn processor(cores: &[(u8, u32)]) -> Option<Cores> {
        cores_from_records(&cores.iter().flat_map(|&(class, threads)| core_record(class, threads)).collect::<Vec<_>>())
    }

    #[test]
    fn prompts_use_every_thread_only_on_alike_cores() {
        // Ryzen 5 1600: 6 cores, 12 threads.
        let ryzen = processor(&[(0, 2); 6]).unwrap();
        assert_eq!(ryzen, Cores { physical: 6, logical: 12, uniform: true });
        assert_eq!(ryzen.prompt_threads(), Some(12));
        // One thread a core: the engine's own choice.
        assert_eq!(processor(&[(0, 1); 4]).unwrap().prompt_threads(), None);
        // 6 performance cores with two threads and 8 efficiency cores with one.
        let hybrid = processor(&[[(1, 2); 6].as_slice(), [(0, 1); 8].as_slice()].concat()).unwrap();
        assert_eq!((hybrid.physical, hybrid.logical, hybrid.uniform), (14, 20, false));
        assert_eq!(hybrid.prompt_threads(), None);
        // Alike cores of another class than 0 are still alike.
        assert_eq!(processor(&[(1, 2); 4]).unwrap().prompt_threads(), Some(8));
        // Nothing, or a cut record, tells nothing.
        assert_eq!(cores_from_records(&[]), None);
        assert_eq!(cores_from_records(&core_record(0, 2)[..40]), None);
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
    fn the_drive_root_of_a_folder() {
        #[cfg(windows)]
        {
            assert_eq!(drive_root(Path::new("d:\\Zaklon\\library")), "D:\\");
            assert_eq!(drive_root(Path::new("\\\\?\\E:\\Zaklon")), "E:\\");
            // A relative folder is on the drive of the current folder.
            assert!(drive_root(Path::new("library")).ends_with(":\\"));
        }
        #[cfg(not(windows))]
        assert_eq!(drive_root(Path::new("/home/zaklon/library")), "/");
    }

    #[test]
    fn fat32_limits_file_size() {
        let d = Drive { path: "E:\\".into(), label: String::new(), kind: "removable", file_system: "FAT32".into(), free: 0, total: 0, system: false };
        assert_eq!(d.max_file_size(), Some(4_294_967_295));
        let n = Drive { file_system: "exFAT".into(), ..d };
        assert_eq!(n.max_file_size(), None);
    }
}
