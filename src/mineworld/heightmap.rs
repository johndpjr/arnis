//! `--heightmap <png>`: terrain from a painted height map instead of downloaded elevation.
//!
//! The PNG is 16-bit greyscale, one pixel per world block, each value the surface's Minecraft
//! Y + 64 (0 = Y -64; the same convention as a WorldPainter 1:1 height map export). Heights are
//! used exactly: the provider hands them over as "metres" and the elevation mapping is fixed at
//! one block per metre with the base at the lowest painted Y, so every column's surface lands on
//! the painted Y. The Earth repairs that target satellite DSM defects are skipped
//! (`SourceMode::Painted` allows no Earth fallback), as is the steep detail (no source pixels).

use crate::coordinate_system::geographic::LLBBox;
use crate::elevation::postprocess::ElevationAffine;
use crate::elevation::provider::{ElevationProvider, RawElevationGrid};
use std::path::Path;
use std::sync::OnceLock;

/// Pixel value of Y 0.
pub const Y_OFFSET: i32 = 64;

struct Painted {
    width: usize,
    height: usize,
    /// Row-major surface Y per block.
    y: Vec<i32>,
    min_y: i32,
    max_y: i32,
}

static PAINTED: OnceLock<Painted> = OnceLock::new();

pub fn load(path: &Path) -> Result<(), String> {
    let err = |e: String| format!("--heightmap {}: {e}", path.display());
    let img = image::ImageReader::open(path)
        .map_err(|e| err(e.to_string()))?
        .decode()
        .map_err(|e| err(e.to_string()))?;
    if img.color() != image::ColorType::L16 {
        return Err(err(format!(
            "must be a 16-bit greyscale PNG of surface Y + {Y_OFFSET} (got {:?})",
            img.color()
        )));
    }
    let gray = img.into_luma16();
    let (width, height) = (gray.width() as usize, gray.height() as usize);
    let y: Vec<i32> = gray
        .into_raw()
        .into_iter()
        .map(|v| i32::from(v) - Y_OFFSET)
        .collect();
    let (min_y, max_y) = y
        .iter()
        .fold((i32::MAX, i32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    PAINTED
        .set(Painted {
            width,
            height,
            y,
            min_y,
            max_y,
        })
        .map_err(|_| err("loaded twice".to_string()))
}

/// The elevation mapping that puts every painted height on its own Y: one block per "metre",
/// with the base at the lowest painted Y.
pub fn affine() -> Option<ElevationAffine> {
    let p = PAINTED.get()?;
    Some(ElevationAffine {
        min_height_m: f64::from(p.min_y),
        blocks_per_meter: 1.0,
        ground_level: p.min_y,
        soft_top: None,
    })
}

/// The painted heights as an elevation source.
pub struct PaintedHeights;

impl ElevationProvider for PaintedHeights {
    fn name(&self) -> &'static str {
        "painted"
    }

    fn native_resolution_m(&self) -> f64 {
        1.0
    }

    fn fetch_raw(
        &self,
        _bbox: &LLBBox,
        grid_width: usize,
        grid_height: usize,
    ) -> Result<RawElevationGrid, Box<dyn std::error::Error>> {
        let p = PAINTED.get().ok_or("no --heightmap loaded")?;
        // One cell per block, as for every world up to Arnis's 16384-block grid cap; anything
        // else means the bbox and the painted map disagree.
        if p.width != grid_width || p.height != grid_height {
            return Err(format!(
                "--heightmap is {} x {} but this world's elevation grid is {} x {}; the painted map \
                 must cover the world block for block",
                p.width, p.height, grid_width, grid_height
            )
            .into());
        }
        println!(
            "Terrain: painted height map ({} x {}), Y {} to {}",
            p.width, p.height, p.min_y, p.max_y
        );
        let heights_meters =
            p.y.chunks(p.width)
                .map(|row| row.iter().map(|&v| f64::from(v)).collect())
                .collect();
        Ok(RawElevationGrid {
            heights_meters,
            source_pixels: None,
        })
    }
}
