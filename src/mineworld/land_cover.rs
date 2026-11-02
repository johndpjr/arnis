//! `--landcover <png>`: land cover from a painted map instead of ESA WorldCover.
//!
//! The PNG is 8-bit greyscale, one pixel per world block (the world's full width and depth),
//! each value an ESA WorldCover class (`LC_*` in `crate::land_cover`), 0 = no data. It is used
//! exactly as painted: the ESA passes that clean up 10 m satellite pixels (shoreline
//! reconstruction, gap filling, boundary smoothing) are skipped. Only the water distance that
//! drives water depth is computed. The passes Arnis runs later (OSM water/land overrides,
//! beaches, bridge repair) still apply.

use crate::coordinate_system::geographic::LLBBox;
use crate::land_cover::{compute_water_distance, LandCoverData};
use once_cell::sync::OnceCell;
use std::path::Path;
use std::sync::OnceLock;

struct Painted {
    width: usize,
    height: usize,
    /// Row-major classes.
    data: Vec<u8>,
}

static PAINTED: OnceLock<Painted> = OnceLock::new();

/// Whether land cover comes from a painted map. The painted map and the OSM water it is drawn
/// with come from the same painting, so the passes that reconcile ESA with OSM must not run:
/// they read OSM areas less their edge blocks and would move the painted shore.
pub fn is_painted() -> bool {
    PAINTED.get().is_some()
}

/// Reads and checks the PNG; `land_cover_for` serves it from then on.
pub fn load(path: &Path) -> Result<(), String> {
    let err = |e: String| format!("--landcover {}: {e}", path.display());
    let img = image::ImageReader::open(path)
        .map_err(|e| err(e.to_string()))?
        .decode()
        .map_err(|e| err(e.to_string()))?;
    if img.color() != image::ColorType::L8 {
        return Err(err(format!(
            "must be an 8-bit greyscale PNG of land-cover classes (got {:?})",
            img.color()
        )));
    }
    let gray = img.into_luma8();
    let painted = Painted {
        width: gray.width() as usize,
        height: gray.height() as usize,
        data: gray.into_raw(),
    };
    PAINTED
        .set(painted)
        .map_err(|_| err("loaded twice".to_string()))
}

/// The painted land cover for a `grid_width` x `grid_height` grid, or None without
/// `--landcover`. The grid must be one cell per block (it is for every world up to Arnis's
/// 16384-block grid cap); anything else means the bbox and the painted map disagree, which
/// is a bug to fail loudly on rather than resample around.
pub fn land_cover_for(
    bbox: &LLBBox,
    grid_width: usize,
    grid_height: usize,
) -> Option<LandCoverData> {
    let p = PAINTED.get()?;
    if p.width != grid_width || p.height != grid_height {
        eprintln!(
            "Error: --landcover is {} x {} but this world's land-cover grid is {} x {}; the painted \
             map must cover the world block for block.",
            p.width, p.height, grid_width, grid_height
        );
        std::process::exit(1);
    }
    let grid: Vec<Vec<u8>> = p.data.chunks(p.width).map(<[u8]>::to_vec).collect();
    let water_distance = compute_water_distance(&grid, p.width, p.height);
    let (_, width_m) =
        crate::coordinate_system::transformation::geo_distance(bbox.min(), bbox.max());
    println!(
        "Land cover: painted map ({} x {}), no download",
        p.width, p.height
    );
    Some(LandCoverData {
        grid,
        water_distance,
        water_blend_cache: OnceCell::new(),
        beach_tiles: OnceCell::new(),
        inland_water: Default::default(),
        width: p.width,
        height: p.height,
        cells_per_meter: if width_m > 0.0 {
            p.width as f64 / width_m
        } else {
            1.0
        },
    })
}
