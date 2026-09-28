//! The Zaklon map's data: vector tiles read from local PMTiles archives and
//! served behind one address, `/tiles/{z}/{x}/{y}.mvt`, together with the
//! fonts and icons the map's style draws labels with.
//!
//! The archives are the map packs from Add-ons (the whole world, 138 GB) and
//! the small world overview that comes with the app (zoom 0-5), in that
//! order: a tile comes from the most detailed archive that has it. The
//! overview, the fonts, the icons and the list of cities are the app's *map
//! assets*, a folder next to the program (see [`map_assets_dir`]).
//!
//! Archives are read with positioned reads on an ordinary file handle, not
//! memory-mapped: Windows then still lets a pack be removed or replaced
//! while the map is open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use pmtiles::{AsyncBackend, AsyncPmTilesReader, BackendResponse, Compression, DirEntry, Directory, DirectoryCache, PmtResult, TileCoord, TileId};

use crate::downloads::Downloads;

/// Where the map assets are: `ZAKLON_MAP_ASSETS` when set (development and
/// tests), else the `map-assets` folder next to the program, where the
/// installer puts them. None when there are none: the map then has no
/// overview, labels or city search until a map pack is installed.
pub fn map_assets_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ZAKLON_MAP_ASSETS").map(PathBuf::from) {
        return dir.is_dir().then_some(dir);
    }
    let dir = std::env::current_exe().ok()?.parent()?.join(MAP_ASSETS_FOLDER);
    dir.is_dir().then_some(dir)
}

/// The map assets' folder next to the program.
pub const MAP_ASSETS_FOLDER: &str = "map-assets";
/// The world overview in the map assets.
pub const OVERVIEW_FILE: &str = "overview.pmtiles";
/// The list of the archives in use is looked at again at most this often.
const RECHECK: Duration = Duration::from_secs(2);
/// Leaf directories kept in memory per archive (the world map has about
/// 350 MB of them on disk; the ones for the area on screen are what counts).
const DIRECTORIES_KEPT: usize = 256;

/// Where an archive comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A map pack from Add-ons.
    Pack,
    /// The world overview that comes with the app.
    Overview,
}

/// One archive to read, as found on disk; a change means it is opened again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    kind: Kind,
    pack: Option<String>,
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

/// An open archive.
pub struct Archive {
    pub kind: Kind,
    pub pack: Option<String>,
    pub path: PathBuf,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// How its tiles are compressed, as an HTTP Content-Encoding.
    pub encoding: Option<&'static str>,
    /// Changes whenever the file does; part of every tile's ETag.
    pub tag: String,
    reader: AsyncPmTilesReader<FileBackend, DirCache>,
}

/// A tile, as it is stored (compressed with `encoding`).
pub struct Tile {
    pub data: Bytes,
    pub encoding: Option<&'static str>,
    pub etag: String,
}

/// What a request for a tile finds.
pub enum Found {
    Tile(Tile),
    /// Inside the map's zoom levels but no archive has it (open sea, say): an
    /// empty tile.
    Empty,
    /// Outside every archive's zoom levels, or not a tile at all.
    Outside,
}

/// A short summary for the app: how detailed the map is right now.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Summary {
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// Changes whenever the archives in use do; the app puts it into the
    /// tiles' address so the browser does not keep tiles of another set.
    pub key: String,
    /// A map pack is in use (not only the overview).
    pub detailed: bool,
    pub overview: bool,
}

#[derive(Default)]
struct Opened {
    sources: Vec<Source>,
    archives: Vec<Arc<Archive>>,
    checked: Option<Instant>,
}

pub struct Tiles {
    downloads: Arc<Downloads>,
    assets: Option<PathBuf>,
    opened: tokio::sync::Mutex<Opened>,
}

impl Tiles {
    pub fn new(downloads: Arc<Downloads>, assets: Option<PathBuf>) -> Arc<Self> {
        if let Some(dir) = &assets {
            tracing::info!(dir = %dir.display(), "map assets");
        }
        Arc::new(Self { downloads, assets, opened: tokio::sync::Mutex::new(Opened::default()) })
    }

    /// The map assets' folder, if there is one.
    pub fn assets(&self) -> Option<&Path> {
        self.assets.as_deref()
    }

    /// The archives on disk right now, most detailed first (by kind; the
    /// order within a kind is settled by zoom once they are open).
    fn sources(&self) -> Vec<Source> {
        let found = |kind: Kind, pack: Option<String>, path: PathBuf| {
            let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
            Some(Source { kind, pack, path, size: meta.len(), modified: meta.modified().ok() })
        };
        let packs = self.downloads.map_archives().into_iter().filter_map(|(id, path)| found(Kind::Pack, Some(id), path));
        let overview = self.assets.as_ref().and_then(|dir| found(Kind::Overview, None, dir.join(OVERVIEW_FILE)));
        packs.chain(overview).collect()
    }

    /// The archives in use, opening them again when what is on disk changed
    /// (a pack was installed, found, updated or removed).
    pub async fn archives(&self) -> Vec<Arc<Archive>> {
        let mut opened = self.opened.lock().await;
        if opened.checked.is_some_and(|t| t.elapsed() < RECHECK) {
            return opened.archives.clone();
        }
        opened.checked = Some(Instant::now());
        // A few file lookups. A map put in place by hand is noticed here too.
        self.downloads.notice_placed_maps();
        let sources = self.sources();
        if sources == opened.sources {
            return opened.archives.clone();
        }
        let mut archives = Vec::new();
        for s in &sources {
            // An archive already open for the same, unchanged file stays open.
            if let Some(a) = opened.archives.iter().find(|a| a.path == s.path && a.kind == s.kind && a.tag == tag_of(s)) {
                archives.push(a.clone());
                continue;
            }
            match Archive::open(s).await {
                Ok(a) => {
                    tracing::info!(path = %s.path.display(), min_zoom = a.min_zoom, max_zoom = a.max_zoom, "map archive opened");
                    archives.push(Arc::new(a));
                }
                Err(e) => tracing::warn!(path = %s.path.display(), "cannot read this map archive: {e}"),
            }
        }
        // The most detailed first; a pack before the overview when equal.
        archives.sort_by_key(|a| (std::cmp::Reverse(a.max_zoom), a.kind != Kind::Pack));
        opened.sources = sources;
        opened.archives = archives.clone();
        archives
    }

    /// Close every archive (they are opened again when next needed).
    pub async fn close_all(&self) {
        *self.opened.lock().await = Opened::default();
    }

    pub async fn summary(&self) -> Summary {
        summarize(&self.archives().await)
    }

    /// The tile at z/x/y from the most detailed archive that has it.
    pub async fn tile(&self, z: u8, x: u32, y: u32) -> Found {
        find_tile(&self.archives().await, z, x, y).await
    }
}

/// Changes whenever the file does: its size and when it was written.
fn tag_of(s: &Source) -> String {
    let stamp = s.modified.and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:x}-{:x}", s.size, stamp)
}

fn summarize(archives: &[Arc<Archive>]) -> Summary {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for a in archives {
        h.update(a.tag.as_bytes());
        h.update([0]);
    }
    let key: String = h.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect();
    Summary {
        min_zoom: archives.iter().map(|a| a.min_zoom).min().unwrap_or(0),
        max_zoom: archives.iter().map(|a| a.max_zoom).max().unwrap_or(0),
        key,
        detailed: archives.iter().any(|a| a.kind == Kind::Pack),
        overview: archives.iter().any(|a| a.kind == Kind::Overview),
    }
}

/// A tile from the first of `archives` (most detailed first) that has it.
pub async fn find_tile(archives: &[Arc<Archive>], z: u8, x: u32, y: u32) -> Found {
    let Ok(coord) = TileCoord::new(z, x, y) else { return Found::Outside };
    let mut inside = false;
    for a in archives {
        if z < a.min_zoom || z > a.max_zoom {
            continue;
        }
        inside = true;
        match a.reader.get_tile(coord).await {
            Ok(Some(data)) if !data.is_empty() => {
                return Found::Tile(Tile { data, encoding: a.encoding, etag: format!("\"{}-{z}-{x}-{y}\"", a.tag) });
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(path = %a.path.display(), z, x, y, "reading a map tile: {e}"),
        }
    }
    if inside {
        Found::Empty
    } else {
        Found::Outside
    }
}

impl Archive {
    async fn open(s: &Source) -> Result<Self, String> {
        let backend = FileBackend::open(&s.path).map_err(|e| e.to_string())?;
        let reader = AsyncPmTilesReader::try_from_cached_source(backend, DirCache::default()).await.map_err(|e| e.to_string())?;
        let h = reader.get_header();
        if h.tile_type != pmtiles::TileType::Mvt {
            return Err(format!("not vector tiles ({:?})", h.tile_type));
        }
        // Gzip (what Protomaps uses) or none: what every web view can take.
        let encoding = match h.tile_compression {
            Compression::Gzip => Some("gzip"),
            Compression::None => None,
            other => return Err(format!("tiles compressed with {other:?}, which older web views cannot read")),
        };
        Ok(Self {
            kind: s.kind,
            pack: s.pack.clone(),
            path: s.path.clone(),
            min_zoom: h.min_zoom,
            max_zoom: h.max_zoom,
            encoding,
            tag: tag_of(s),
            reader,
        })
    }
}

/// Reads an archive with positioned reads, off the async threads.
struct FileBackend {
    file: Arc<std::fs::File>,
    len: u64,
}

impl FileBackend {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file: Arc::new(file), len })
    }
}

impl AsyncBackend for FileBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        let (file, offset) = (self.file.clone(), offset as u64);
        // Up to the end of the file: the first read asks for more than a small archive has.
        let length = length.min(self.len.saturating_sub(offset) as usize);
        let bytes = tokio::task::spawn_blocking(move || read_at(&file, offset, length))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))??;
        Ok(BackendResponse::new(Bytes::from(bytes)))
    }
}

/// Read `length` bytes at `offset` (fewer only at the end of the file).
fn read_at(file: &std::fs::File, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; length];
    let mut filled = 0;
    while filled < length {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(file, &mut buf[filled..], offset + filled as u64)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(file, &mut buf[filled..], offset + filled as u64)?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

/// The archive's leaf directories used last, a limited number of them.
#[derive(Default)]
struct DirCache {
    inner: Mutex<DirCacheInner>,
}

#[derive(Default)]
struct DirCacheInner {
    dirs: HashMap<usize, (Arc<Directory>, u64)>,
    tick: u64,
}

impl DirCache {
    fn get(&self, offset: usize) -> Option<Arc<Directory>> {
        let mut c = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        c.tick += 1;
        let tick = c.tick;
        c.dirs.get_mut(&offset).map(|(d, used)| {
            *used = tick;
            d.clone()
        })
    }

    fn put(&self, offset: usize, dir: Arc<Directory>) {
        let mut c = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        c.tick += 1;
        let tick = c.tick;
        if c.dirs.len() >= DIRECTORIES_KEPT && !c.dirs.contains_key(&offset) {
            if let Some(oldest) = c.dirs.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| *k) {
                c.dirs.remove(&oldest);
            }
        }
        c.dirs.insert(offset, (dir, tick));
    }
}

impl DirectoryCache for DirCache {
    async fn get_dir_entry_or_insert(
        &self,
        offset: usize,
        tile_id: TileId,
        fetcher: impl std::future::Future<Output = PmtResult<Directory>> + Send,
    ) -> PmtResult<Option<DirEntry>> {
        if let Some(dir) = self.get(offset) {
            return Ok(dir.find_tile_id(tile_id).cloned());
        }
        let dir = Arc::new(fetcher.await?);
        let entry = dir.find_tile_id(tile_id).cloned();
        self.put(offset, dir);
        Ok(entry)
    }
}

/// The font whose glyphs stand in where another font has none of its own.
pub const FALLBACK_FONT: &str = "Noto Sans Regular";

/// A font file a map style asks for: `<fontstack>/<start>-<end>.pbf`, where
/// the font's name is letters, digits, spaces and a little punctuation.
pub fn glyph_path(assets: &Path, fontstack: &str, range: &str) -> Option<PathBuf> {
    let name_ok = !fontstack.is_empty()
        && fontstack.len() <= 100
        && fontstack.chars().all(|c| c.is_ascii_alphanumeric() || " -_".contains(c))
        && !fontstack.starts_with(' ');
    let range_ok = range.strip_suffix(".pbf").and_then(|r| r.split_once('-')).is_some_and(|(a, b)| {
        !a.is_empty() && !b.is_empty() && a.len() <= 5 && b.len() <= 5 && a.chars().chain(b.chars()).all(|c| c.is_ascii_digit())
    });
    (name_ok && range_ok).then(|| assets.join("fonts").join(fontstack).join(range))
}

/// An icon sheet of the map style: `<name>[@2x].json|png`.
pub fn sprite_path(assets: &Path, file: &str) -> Option<PathBuf> {
    let (stem, ext) = file.rsplit_once('.')?;
    let name = stem.strip_suffix("@2x").unwrap_or(stem);
    let ok = matches!(ext, "json" | "png")
        && !name.is_empty()
        && name.len() <= 40
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    ok.then(|| assets.join("sprites").join(file))
}

#[cfg(test)]
pub(crate) mod test_archive {
    //! Tiny map archives for tests, written with the pmtiles crate.

    use std::path::Path;

    /// Write a vector-tile archive with a tile for each of `tiles` whose
    /// data is the given text (gzip-compressed, as Protomaps does it).
    pub fn write(path: &Path, min_zoom: u8, max_zoom: u8, tiles: &[((u8, u32, u32), &str)]) {
        let mut tiles = tiles.to_vec();
        tiles.sort_by_key(|((z, x, y), _)| pmtiles::TileId::from(pmtiles::TileCoord::new(*z, *x, *y).unwrap()).value());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(path).unwrap();
        let mut w = pmtiles::PmTilesWriter::new(pmtiles::TileType::Mvt)
            .tile_compression(pmtiles::Compression::Gzip)
            .min_zoom(min_zoom)
            .max_zoom(max_zoom)
            .create(file)
            .unwrap();
        for ((z, x, y), data) in tiles {
            w.add_tile(pmtiles::TileCoord::new(z, x, y).unwrap(), data.as_bytes()).unwrap();
        }
        w.finalize().unwrap();
    }

    /// Undo the gzip of a served tile.
    pub fn gunzip(data: &[u8]) -> String {
        use std::io::Read;
        let mut out = String::new();
        flate2::read::GzDecoder::new(data).read_to_string(&mut out).unwrap();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_archive::{gunzip, write};
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zaklon-tiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn open(path: &Path, kind: Kind) -> Arc<Archive> {
        let meta = std::fs::metadata(path).unwrap();
        let s = Source { kind, pack: None, path: path.to_path_buf(), size: meta.len(), modified: meta.modified().ok() };
        Arc::new(Archive::open(&s).await.unwrap())
    }

    fn text(found: Found) -> Option<String> {
        match found {
            Found::Tile(t) => {
                assert_eq!(t.encoding, Some("gzip"));
                Some(gunzip(&t.data))
            }
            Found::Empty => Some(String::new()),
            Found::Outside => None,
        }
    }

    #[tokio::test]
    async fn a_tile_comes_from_the_most_detailed_archive_that_has_it() {
        let dir = temp("find");
        let overview = dir.join("overview.pmtiles");
        write(&overview, 0, 1, &[((0, 0, 0), "overview 0"), ((1, 0, 0), "overview 1/0/0"), ((1, 1, 1), "overview 1/1/1")]);
        let world = dir.join("world.pmtiles");
        // The pack has more zoom levels, but not every tile (1/1/1 is missing).
        write(&world, 0, 3, &[((0, 0, 0), "world 0"), ((1, 0, 0), "world 1/0/0"), ((3, 4, 2), "world 3/4/2")]);
        let archives = vec![open(&world, Kind::Pack).await, open(&overview, Kind::Overview).await];
        assert_eq!(text(find_tile(&archives, 0, 0, 0).await).unwrap(), "world 0");
        assert_eq!(text(find_tile(&archives, 1, 1, 1).await).unwrap(), "overview 1/1/1", "falls back tile by tile");
        assert_eq!(text(find_tile(&archives, 3, 4, 2).await).unwrap(), "world 3/4/2");
        assert_eq!(text(find_tile(&archives, 3, 0, 0).await).unwrap(), "", "inside the zoom levels: an empty tile");
        assert!(text(find_tile(&archives, 4, 0, 0).await).is_none(), "beyond every archive");
        assert!(text(find_tile(&archives, 1, 2, 0).await).is_none(), "not a tile at zoom 1");
        let s = summarize(&archives);
        assert_eq!((s.min_zoom, s.max_zoom, s.detailed, s.overview), (0, 3, true, true));
        assert_eq!(s.key.len(), 12);
        assert_ne!(s.key, summarize(&archives[1..]).key, "another set of archives, another key");
        // Only the overview.
        let only = &archives[1..];
        assert_eq!(text(find_tile(only, 1, 0, 0).await).unwrap(), "overview 1/0/0");
        assert!(text(find_tile(only, 3, 4, 2).await).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_open_archive_does_not_keep_its_file_from_being_deleted() {
        let dir = temp("delete");
        let path = dir.join("world.pmtiles");
        write(&path, 0, 0, &[((0, 0, 0), "world")]);
        let a = open(&path, Kind::Pack).await;
        std::fs::remove_file(&path).expect("a pack can be removed while the map is open");
        drop(a);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_the_directories_used_last() {
        let c = DirCache::default();
        let empty = || Arc::new(Directory::default());
        for i in 0..DIRECTORIES_KEPT {
            c.put(i, empty());
        }
        assert!(c.get(0).is_some(), "used again: kept");
        c.put(DIRECTORIES_KEPT, empty());
        assert!(c.get(0).is_some());
        assert!(c.get(1).is_none(), "the one unused longest went");
        assert_eq!(c.inner.lock().unwrap().dirs.len(), DIRECTORIES_KEPT);
    }

    #[test]
    fn style_files_stay_inside_the_assets() {
        let a = Path::new("assets");
        assert_eq!(glyph_path(a, "Noto Sans Regular", "0-255.pbf"), Some(a.join("fonts").join("Noto Sans Regular").join("0-255.pbf")));
        assert!(glyph_path(a, "Noto Sans Devanagari Regular v1", "2304-2559.pbf").is_some());
        for (stack, range) in [("..", "0-255.pbf"), ("a/b", "0-255.pbf"), ("a\\b", "0-255.pbf"), ("C:x", "0-255.pbf"), ("Noto", "0-255"), ("Noto", "../0-255.pbf"), ("Noto", "-255.pbf"), ("", "0-255.pbf")] {
            assert!(glyph_path(a, stack, range).is_none(), "{stack} {range}");
        }
        assert_eq!(sprite_path(a, "dark@2x.png"), Some(a.join("sprites").join("dark@2x.png")));
        assert!(sprite_path(a, "dark.json").is_some());
        for f in ["../dark.json", "dark.svg", "Dark.json", ".json", "dark", "a/b.png"] {
            assert!(sprite_path(a, f).is_none(), "{f}");
        }
    }
}
