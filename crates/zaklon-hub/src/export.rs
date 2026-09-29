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
    /// The folder it goes into (the packs folder, or the drive itself for the apps).
    dir: PathBuf,
    name: String,
    size: u64,
}

const USB_README: &str = "ZAKLON\r\n\
\r\n\
English\r\n\
1. On a Windows computer, run Zaklon-setup.exe (no internet needed).\r\n\
2. Open Zaklon > Add-ons > Import from USB, and choose this drive.\r\n\
3. Phones: copy zaklon.apk to the phone and open it to install, or install\r\n\
   it from the new hub (Household > Add a phone).\r\n\
\r\n\
Srpski\r\n\
1. Na Windows računaru pokreni Zaklon-setup.exe (internet nije potreban).\r\n\
2. Otvori Zaklon > Dodaci > Uvoz sa USB-a i izaberi ovaj disk.\r\n\
3. Telefoni: prebaci zaklon.apk na telefon i otvori ga da se instalira, ili\r\n\
   ga instaliraj sa novog huba (Domaćinstvo > Dodaj telefon).\r\n\
\r\n\
https://zaklon.com\r\n";

/// The apps a friend needs to start from this stick: the Windows installer
/// (kept by the installer next to the data) and the phone app.
pub fn app_files(downloads: &Downloads) -> Vec<(PathBuf, String)> {
    let lib = downloads.library_dir();
    [(lib.join("installer").join("Zaklon-setup.exe"), "Zaklon-setup.exe"), (lib.join("apk").join("zaklon.apk"), "zaklon.apk")]
        .into_iter()
        .filter(|(p, _)| p.is_file())
        .map(|(p, n)| (p, n.to_string()))
        .collect()
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
    pub fn start(self: &Arc<Self>, downloads: &Downloads, ids: &[String], dir: &Path, with_apps: bool) -> Result<(), String> {
        // Claim the exporter first, in one step, so a double click cannot start two copies.
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if st.running {
                return Err("a copy is already running".into());
            }
            *st = ExportState { running: true, ..Default::default() };
        }
        let result = self.prepare_and_spawn(downloads, ids, dir, with_apps);
        if result.is_err() {
            self.set(|s| s.running = false);
        }
        result
    }

    fn prepare_and_spawn(self: &Arc<Self>, downloads: &Downloads, ids: &[String], dir: &Path, with_apps: bool) -> Result<(), String> {
        let apps = if with_apps { app_files(downloads) } else { Vec::new() };
        if ids.is_empty() && apps.is_empty() {
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
        let [world, coasts] = zaklon_core::maps::BASE_IDS;
        for (cat, dep) in [(Category::Knowledge, "kiwix-tools"), (Category::Maps, zaklon_core::maps::COMAPS_APK_ID), (Category::Maps, world), (Category::Maps, coasts)] {
            if cats.contains(&cat) && downloads.is_installed(dep) && !ids.iter().any(|i| i == dep) {
                ids.push(dep.to_string());
            }
        }
        let mut jobs = Vec::new();
        for id in &ids {
            let pack = downloads.catalog().pack(id).cloned().ok_or("unknown pack")?;
            // The files on disk as verified (possibly an older version than the catalog's).
            let files = downloads.installed_files(id);
            if files.is_empty() {
                return Err(format!("{} is not installed", pack.title.en));
            }
            for f in &files {
                let src = downloads.library_dir().join(&f.path);
                if !src.is_file() {
                    return Err(format!("{} cannot be copied", pack.title.en));
                }
                let name = Path::new(&f.path).file_name().ok_or("bad path")?.to_string_lossy().to_string();
                jobs.push(Job { src, dir: target.clone(), name, size: f.size });
            }
        }
        for (src, name) in &apps {
            let size = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
            jobs.push(Job { src: src.clone(), dir: dir.to_path_buf(), name: name.clone(), size });
        }
        // Files already there with the right size are skipped (a repeated copy).
        let needed: u64 = jobs.iter().filter(|j| !same_size(&j.dir.join(&j.name), j.size)).map(|j| j.size).sum();
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
        if !apps.is_empty() {
            let _ = std::fs::write(dir.join("ZAKLON-README.txt"), USB_README);
        }
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
            // A panic must still end the copy, or every later one would hear
            // "a copy is already running".
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.run(&jobs)))
                .unwrap_or_else(|_| Err("the copy stopped unexpectedly".into()));
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

    fn run(&self, jobs: &[Job]) -> Result<(), String> {
        for j in jobs {
            let dest = j.dir.join(&j.name);
            self.set(|s| s.current = Some(j.name.clone()));
            if same_size(&dest, j.size) {
                self.set(|s| {
                    s.bytes_done += j.size;
                    s.files_done += 1;
                });
                continue;
            }
            let part = j.dir.join(format!("{}.part", j.name));
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
                return Err("canceled".into());
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
