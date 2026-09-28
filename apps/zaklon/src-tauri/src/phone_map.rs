//! The map's assets the phone app carries (see build.rs): the world
//! overview (zoom 0-5), the glyphs of the map's font and the style's icons.
//! A phone away from home still shows the world with them, and at home its
//! own copy spares the Wi-Fi. The content proxy (client.rs) asks here first
//! and the hub for the rest (the world map's closer zoom levels).

use bytes::Bytes;
use pmtiles::{AsyncBackend, AsyncPmTilesReader, BackendResponse, Compression, PmtResult, TileCoord};
use serde::Serialize;

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/map_assets.rs"));
}

type Files = &'static [(&'static str, &'static [u8])];

/// What the phone has of the map, for the app (instead of /api/map away from home).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalInfo {
    pub overview: bool,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub glyphs: bool,
    pub sprites: bool,
}

/// An answer the phone has itself.
#[derive(Debug, PartialEq, Eq)]
pub enum Local {
    /// A tile, gzip-compressed as it is stored.
    Tile(Bytes),
    /// Inside the overview's zoom levels, but it has no tile there (the open sea).
    Empty,
    /// A glyph range or an icon sheet, and its content type.
    File(&'static [u8], &'static str),
}

struct MemBackend(&'static [u8]);

impl AsyncBackend for MemBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        let start = offset.min(self.0.len());
        let end = offset.saturating_add(length).min(self.0.len());
        Ok(BackendResponse::new(Bytes::from_static(&self.0[start..end])))
    }
}

struct Overview {
    reader: AsyncPmTilesReader<MemBackend>,
    min_zoom: u8,
    max_zoom: u8,
}

pub struct PhoneMap {
    overview: Option<Overview>,
    glyphs: Files,
    sprites: Files,
}

impl PhoneMap {
    /// What this build of the app carries (nothing on a laptop).
    pub async fn bundled() -> Self {
        Self::new(bundled::OVERVIEW, bundled::GLYPHS, bundled::SPRITES).await
    }

    pub async fn new(overview: &'static [u8], glyphs: Files, sprites: Files) -> Self {
        let overview = if overview.is_empty() {
            None
        } else {
            match AsyncPmTilesReader::try_from_source(MemBackend(overview)).await {
                Ok(reader) if reader.get_header().tile_compression == Compression::Gzip => {
                    let h = reader.get_header();
                    let (min_zoom, max_zoom) = (h.min_zoom, h.max_zoom);
                    Some(Overview { reader, min_zoom, max_zoom })
                }
                Ok(_) => {
                    tracing::warn!("the map overview in the app is not gzip-compressed; not used");
                    None
                }
                Err(e) => {
                    tracing::warn!("the map overview in the app cannot be read: {e}");
                    None
                }
            }
        };
        Self { overview, glyphs, sprites }
    }

    pub fn info(&self) -> LocalInfo {
        LocalInfo {
            overview: self.overview.is_some(),
            min_zoom: self.overview.as_ref().map_or(0, |o| o.min_zoom),
            max_zoom: self.overview.as_ref().map_or(0, |o| o.max_zoom),
            glyphs: !self.glyphs.is_empty(),
            sprites: !self.sprites.is_empty(),
        }
    }

    /// The phone's own answer to a map request (the path after the proxy's
    /// secret, with or without a query), or None when the hub must answer.
    /// Glyphs of any of the style's fonts come from the regular one.
    pub async fn answer(&self, path: &str) -> Option<Local> {
        let path = path.split('?').next().unwrap_or(path);
        let last = path.rsplit('/').next().unwrap_or_default();
        if path.starts_with("/map/fonts/") {
            return self.glyphs.iter().find(|(name, _)| *name == last).map(|&(_, data)| Local::File(data, "application/x-protobuf"));
        }
        if path.starts_with("/map/sprites/") {
            let kind = if last.ends_with(".png") { "image/png" } else { "application/json" };
            return self.sprites.iter().find(|(name, _)| *name == last).map(|&(_, data)| Local::File(data, kind));
        }
        let rest = path.strip_prefix("/tiles/")?;
        let mut parts = rest.split('/');
        let z: u8 = parts.next()?.parse().ok()?;
        let x: u32 = parts.next()?.parse().ok()?;
        let y: u32 = parts.next()?.strip_suffix(".mvt")?.parse().ok()?;
        let o = self.overview.as_ref()?;
        if z < o.min_zoom || z > o.max_zoom || parts.next().is_some() {
            return None;
        }
        let coord = TileCoord::new(z, x, y).ok()?;
        match o.reader.get_tile(coord).await {
            Ok(Some(data)) if !data.is_empty() => Some(Local::Tile(data)),
            Ok(_) => Some(Local::Empty),
            Err(e) => {
                tracing::warn!(z, x, y, "reading a tile of the app's map overview: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interface tests' tiny overview: the world at zoom 0-1.
    static OVERVIEW: &[u8] = include_bytes!("../../../../ui/e2e/fixtures/map-assets/overview.pmtiles");
    static GLYPHS: Files = &[("0-255.pbf", b"glyphs" as &[u8])];
    static SPRITES: Files = &[("dark.json", b"{}" as &[u8]), ("dark@2x.png", b"png" as &[u8])];

    #[tokio::test]
    async fn answers_what_the_app_carries_and_leaves_the_rest_to_the_hub() {
        let m = PhoneMap::new(OVERVIEW, GLYPHS, SPRITES).await;
        assert_eq!(m.info(), LocalInfo { overview: true, min_zoom: 0, max_zoom: 1, glyphs: true, sprites: true });
        let Some(Local::Tile(t)) = m.answer("/tiles/0/0/0.mvt?v=abc").await else { panic!("the overview's tile") };
        assert_eq!(&t[..2], &[0x1f, 0x8b], "gzip, as stored");
        assert!(matches!(m.answer("/tiles/1/1/0.mvt").await, Some(Local::Tile(_))));
        assert_eq!(m.answer("/tiles/2/0/0.mvt").await, None, "closer in: the hub's world map");
        assert_eq!(m.answer("/tiles/1/2/0.mvt").await, None, "not a tile");
        assert_eq!(m.answer("/tiles/0/0/0.png").await, None);
        // Any of the style's fonts is drawn with the regular one.
        assert_eq!(m.answer("/map/fonts/Noto%20Sans%20Medium/0-255.pbf").await, Some(Local::File(b"glyphs", "application/x-protobuf")));
        assert_eq!(m.answer("/map/fonts/Noto%20Sans%20Regular/256-511.pbf").await, None);
        assert_eq!(m.answer("/map/sprites/dark@2x.png").await, Some(Local::File(b"png", "image/png")));
        assert_eq!(m.answer("/map/sprites/dark.json").await, Some(Local::File(b"{}", "application/json")));
        assert_eq!(m.answer("/kiwix/content/x").await, None);
    }

    #[tokio::test]
    async fn without_assets_the_hub_answers_everything() {
        let m = PhoneMap::new(&[], &[], &[]).await;
        assert!(!m.info().overview);
        assert_eq!(m.answer("/tiles/0/0/0.mvt").await, None);
        assert_eq!(m.answer("/map/fonts/Noto%20Sans%20Regular/0-255.pbf").await, None);
        // A laptop's app carries none (the hub has its own).
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        assert!(!PhoneMap::bundled().await.info().overview);
    }
}
