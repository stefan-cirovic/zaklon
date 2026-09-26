//! Copying installed packs to a folder, usually a USB drive, so another
//! Zaklon can import them without internet ("Copy to USB" on Add-ons).
//! Runs in the background with progress; one copy at a time.
//!
//! Files land in `<folder>\zaklon-packs\` under their own names, which is
//! where "Import from USB" looks. Each file is written as `.part` and renamed
//! when complete, so an unplugged drive never leaves a file that looks whole.
//! The importing hub checks every file against the catalog's hash anyway.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use zaklon_core::catalog::Category;

use crate::downloads::Downloads;

pub const FOLDER: &str = "zaklon-packs";

const README: &str = "These files are Zaklon packs (offline knowledge, maps, AI models).\r\n\
\r\n\
To use them on another computer with Zaklon: open Zaklon, go to Add-ons,\r\n\
and under \"Import from USB\" choose this drive. Zaklon checks every file\r\n\
before using it. Nothing here needs internet.\r\n\
\r\n\
More about Zaklon: https://zaklon.com\r\n";

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExportState {
    pub running: bool,
    /// The folder files are copied into.
    pub target: Option<String>,
    pub current: Option<String>,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub error: Option<String>,
    /// Set when the last copy finished without error.
    pub finished: bool,
}

#[derive(Default)]
pub struct Exporter {
    state: Mutex<ExportState>,
    cancel: AtomicBool,
}

struct Job {
    src: PathBuf,
    name: String,
    size: u64,
}

impl Exporter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn state(&self) -> ExportState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn set(&self, f: impl FnOnce(&mut ExportState)) {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()));
    }

    /// Checks everything that can be checked up front (folder, packs, space,
    /// file size limit of the drive) and starts copying in the background.
    pub fn start(self: &Arc<Self>, downloads: &Downloads, ids: &[String], dir: &Path) -> Result<(), String> {
        if self.state().running {
            return Err("a copy is already running".into());
        }
        if ids.is_empty() {
            return Err("nothing selected".into());
        }
        if !dir.is_dir() {
            return Err("that folder does not exist".into());
        }
        let target = dir.join(FOLDER);
        if target.starts_with(downloads.library_dir()) {
            return Err("choose a folder outside the library".into());
        }
        // What the packs need to work on the other computer comes along:
        // the library engine for knowledge packs, the CoMaps app for maps.
        let mut ids: Vec<String> = ids.to_vec();
        let cats: Vec<Category> = ids.iter().filter_map(|id| downloads.catalog().pack(id).map(|p| p.category.clone())).collect();
        for (cat, dep) in [(Category::Knowledge, "kiwix-tools"), (Category::Maps, zaklon_core::maps::COMAPS_APK_ID)] {
            if cats.contains(&cat) && downloads.is_installed(dep) && !ids.iter().any(|i| i == dep) {
                ids.push(dep.to_string());
            }
        }
        let mut jobs = Vec::new();
        for id in &ids {
            let pack = downloads.catalog().pack(id).ok_or("unknown pack")?;
            if !downloads.is_installed(id) {
                return Err(format!("{} is not installed", pack.title.en));
            }
            for f in &pack.files {
                let src = downloads.library_dir().join(&f.path);
                if !src.is_file() {
                    return Err(format!("{} cannot be copied", pack.title.en));
                }
                let name = Path::new(&f.path).file_name().ok_or("bad path")?.to_string_lossy().to_string();
                jobs.push(Job { src, name, size: f.size });
            }
        }
        // Files already there with the right size are skipped (a repeated copy).
        let needed: u64 = jobs.iter().filter(|j| !same_size(&target.join(&j.name), j.size)).map(|j| j.size).sum();
        let free = fs4::available_space(dir).unwrap_or(u64::MAX);
        if needed > free {
            return Err(format!("not enough space: {needed} bytes needed, {free} free"));
        }
        if let Some(limit) = crate::machine::drive_of(dir).and_then(|d| d.max_file_size()) {
            if jobs.iter().any(|j| j.size > limit) {
                return Err("this drive is formatted as FAT32, which cannot hold files of 4 GB or more; format it as exFAT or NTFS".into());
            }
        }
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        let _ = std::fs::write(target.join("README.txt"), README);
        self.cancel.store(false, Ordering::SeqCst);
        let total = jobs.iter().map(|j| j.size).sum();
        self.set(|s| {
            *s = ExportState {
                running: true,
                target: Some(target.display().to_string()),
                files_total: jobs.len(),
                bytes_total: total,
                ..Default::default()
            }
        });
        let me = self.clone();
        std::thread::spawn(move || {
            let result = me.run(&jobs, &target);
            me.set(|s| {
                s.running = false;
                s.current = None;
                match result {
                    Ok(()) => s.finished = true,
                    Err(e) => s.error = Some(e),
                }
            });
        });
        Ok(())
    }

    fn run(&self, jobs: &[Job], target: &Path) -> Result<(), String> {
        for j in jobs {
            let dest = target.join(&j.name);
            self.set(|s| s.current = Some(j.name.clone()));
            if same_size(&dest, j.size) {
                self.set(|s| {
                    s.bytes_done += j.size;
                    s.files_done += 1;
                });
                continue;
            }
            let part = target.join(format!("{}.part", j.name));
            self.copy(&j.src, &part).inspect_err(|_| {
                let _ = std::fs::remove_file(&part);
            })?;
            std::fs::rename(&part, &dest).map_err(|e| format!("finishing {}: {e}", j.name))?;
            self.set(|s| s.files_done += 1);
        }
        Ok(())
    }

    fn copy(&self, src: &Path, dest: &Path) -> Result<(), String> {
        let mut input = std::fs::File::open(src).map_err(|e| format!("reading {}: {e}", src.display()))?;
        let mut output = std::fs::File::create(dest).map_err(|e| format!("writing to the drive: {e}"))?;
        let mut buf = vec![0u8; 4 << 20];
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err("cancelled".into());
            }
            let n = input.read(&mut buf).map_err(|e| format!("reading {}: {e}", src.display()))?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n]).map_err(|e| format!("writing to the drive: {e}"))?;
            self.set(|s| s.bytes_done += n as u64);
        }
        // Make sure it is really on the stick before saying "done".
        output.sync_all().map_err(|e| format!("writing to the drive: {e}"))?;
        Ok(())
    }
}

fn same_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).map(|m| m.is_file() && m.len() == size).unwrap_or(false)
}
