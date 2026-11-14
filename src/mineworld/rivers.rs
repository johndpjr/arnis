//! `--rivers <png>`: rivers carved by the front end, filled to the surface it planned.
//!
//! Arnis's own water does not suit a river drawn on painted terrain: its land-cover repair
//! levels every water body to one height and pulls the land within 25 blocks down towards it,
//! so a river falling down a valley becomes a trench or a flooded slope. Mineworld plans the
//! river itself (a water surface that never rises downstream, stepping down a block at a time;
//! a bed below it; sloped banks), writes the bed into the height map, and marks the river's
//! blocks as bare land in the land cover so none of Arnis's water passes touch them. This pass
//! then puts the water in, after the ground is built:
//!
//! - the PNG is 16-bit greyscale, one pixel per world block (x east, z south, from the world's
//!   north-west corner), each value the river's water surface Y + 64, 0 where there is no river;
//! - on each river block the ground's top becomes a river bed (gravel, sand, some clay), water
//!   fills from just above it up to the surface, and loose plants or snow just above the water
//!   are cleared. The bed is the lower of the carved one and Arnis's ground: a road graded across
//!   the river (raised on its embankment) goes under the water as a ford instead of damming it,
//!   and where a lake's shore pull-down lowered the ground near its mouth the river is deeper;
//! - a 4 x 4 biome cell holding river blocks takes the river biome (frozen where it would be
//!   cold), like Arnis's lakes and rivers.
//!
//! A world big enough to be built in tiles is filled tile by tile (`fill_area`, right after each
//! tile's own water pass): such a world streams each region to disk as soon as its tiles have
//! merged, so a pass over the merged world would write water into regions already saved.

use crate::biome::Cell;
use crate::block_definitions::{Block, AIR, CLAY, GRAVEL, SAND, SNOW_LAYERS, WATER};
use crate::land_cover::LC_WATER;
use crate::world_editor::WorldEditor;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// Pixel value of Y 0, as in the height map.
const Y_OFFSET: i32 = 64;
/// How far above the water loose plants and snow are cleared.
const CLEAR_ABOVE: i32 = 2;

struct Painted {
    width: usize,
    height: usize,
    /// Surface Y + 64 per block, 0 = none.
    data: Vec<u16>,
}

static PAINTED: OnceLock<Painted> = OnceLock::new();

pub fn load(path: &Path) -> Result<(), String> {
    let err = |e: String| format!("--rivers {}: {e}", path.display());
    let img = image::ImageReader::open(path)
        .map_err(|e| err(e.to_string()))?
        .decode()
        .map_err(|e| err(e.to_string()))?;
    if img.color() != image::ColorType::L16 {
        return Err(err(format!(
            "must be a 16-bit greyscale PNG of water surface Y + {Y_OFFSET} (got {:?})",
            img.color()
        )));
    }
    let gray = img.into_luma16();
    let (width, height) = (gray.width() as usize, gray.height() as usize);
    PAINTED
        .set(Painted {
            width,
            height,
            data: gray.into_raw(),
        })
        .map_err(|_| err("loaded twice".to_string()))
}

fn surface_in(p: &Painted, x: i32, z: i32) -> Option<i32> {
    if x < 0 || z < 0 || x as usize >= p.width || z as usize >= p.height {
        return None;
    }
    match p.data[z as usize * p.width + x as usize] {
        0 => None,
        v => Some(i32::from(v) - Y_OFFSET),
    }
}

/// A biome cell centred on (x, z) is inland water when any block of its 4 x 4 is river, so a
/// narrow river still gets the river biome along its whole length.
pub fn cell(mut c: Cell, x: i32, z: i32) -> Cell {
    let Some(p) = PAINTED.get() else {
        return c;
    };
    let any = (-2..2).any(|dz| (-2..2).any(|dx| surface_in(p, x + dx, z + dz).is_some()));
    if any {
        c.lc = LC_WATER;
        c.inland = true;
    }
    c
}

/// Bed material: mostly gravel, with sand and some clay, fixed per block.
fn bed_block(x: i32, z: i32) -> Block {
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (z as u32).wrapping_mul(0x85EB_CA77);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    match (h >> 24) % 20 {
        0..=11 => GRAVEL,
        12..=17 => SAND,
        _ => CLAY,
    }
}

/// River blocks filled so far, across tiles (reported once, after the merge).
static FILLED: AtomicUsize = AtomicUsize::new(0);

/// Fills the rivers in the whole world, on an editor that holds all of it (the single-pass path).
pub fn fill(editor: &mut WorldEditor) {
    let Some(p) = PAINTED.get() else {
        return;
    };
    fill_area(editor, 0, p.width as i32 - 1, 0, p.height as i32 - 1);
    report();
}

/// After the merge: fills the whole world when the ground was built on the merged editor;
/// otherwise each tile filled its own area (`fill_area`) before its regions could be written to
/// disk, and only the total is reported. Filling after the merge would miss every region a big
/// world has already streamed out.
pub fn fill_merged(editor: &mut WorldEditor, ground_on_merged: bool) {
    if ground_on_merged {
        fill(editor);
    } else {
        report();
    }
}

fn report() {
    let blocks = FILLED.swap(0, Ordering::Relaxed);
    if blocks > 0 {
        println!("Rivers: filled {blocks} blocks of river");
    }
}

/// Fills the river blocks in x min_x..=max_x, z min_z..=max_z (a tile's own area, after its
/// ground and water passes).
pub fn fill_area(editor: &mut WorldEditor, min_x: i32, max_x: i32, min_z: i32, max_z: i32) {
    let Some(p) = PAINTED.get() else {
        return;
    };
    let replace_any: Option<&[Block]> = Some(&[]);
    let mut blocks = 0usize;
    for z in min_z.max(0)..=max_z.min(p.height as i32 - 1) {
        for x in min_x.max(0)..=max_x.min(p.width as i32 - 1) {
            let Some(top) = surface_in(p, x, z) else {
                continue;
            };
            let ground = editor.get_ground_level(x, z);
            let bed = crate::mineworld::heightmap::y_at(x, z).map_or(ground, |y| y.min(ground));
            if bed >= top {
                continue;
            }
            editor.set_block_absolute(bed_block(x, z), x, bed, z, None, replace_any);
            for y in bed + 1..=top {
                editor.set_block_absolute(WATER, x, y, z, None, replace_any);
            }
            // A road embankment standing above the water would still dam the river.
            for y in top + 1..=ground {
                editor.set_block_absolute(AIR, x, y, z, None, replace_any);
            }
            for y in top + 1..=top + CLEAR_ABOVE {
                if editor.get_block_absolute(x, y, z).is_some_and(|b| loose(b)) {
                    editor.set_block_absolute(AIR, x, y, z, None, replace_any);
                }
            }
            blocks += 1;
        }
    }
    FILLED.fetch_add(blocks, Ordering::Relaxed);
}

/// Plants and snow that would float on or stand in the water.
fn loose(b: Block) -> bool {
    crate::ground_decoration::is_undergrowth(b) || SNOW_LAYERS.contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(w: usize, h: usize, at: &[(usize, usize, i32)]) -> Painted {
        let mut data = vec![0u16; w * h];
        for &(x, z, y) in at {
            data[z * w + x] = (y + Y_OFFSET) as u16;
        }
        Painted {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn surface_reads_y_and_nothing_outside() {
        let p = painted(4, 4, &[(1, 2, 70), (3, 3, -5)]);
        assert_eq!(surface_in(&p, 1, 2), Some(70));
        assert_eq!(surface_in(&p, 3, 3), Some(-5));
        assert_eq!(surface_in(&p, 0, 0), None);
        assert_eq!(surface_in(&p, -1, 2), None);
        assert_eq!(surface_in(&p, 4, 2), None);
    }

    #[test]
    fn the_bed_is_mostly_gravel_and_fixed_per_block() {
        let mut gravel = 0;
        for x in 0..50 {
            for z in 0..50 {
                assert_eq!(bed_block(x, z), bed_block(x, z));
                if bed_block(x, z) == GRAVEL {
                    gravel += 1;
                }
            }
        }
        assert!((1200..1800).contains(&gravel), "{gravel} of 2500");
    }
}
