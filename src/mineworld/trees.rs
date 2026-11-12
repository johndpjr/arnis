//! `--tree-clearance <blocks>`: trees keep clear of buildings and roads.
//!
//! Arnis keeps trees off building footprints, but its footprint bitmap is the polygon fill, which
//! leaves out the outline (the fill excludes cells on an edge; buildings draw their walls there).
//! Trees planted before the walls went up therefore stood on wall lines, and their leaves, culled
//! only in footprint columns, filled the rooms behind them. A forest painted over a town shows it
//! everywhere.
//!
//! With the flag:
//! - the outline of every building (ways and relation members) is added to the footprint bitmap
//!   before anything is planted, so Arnis's own rules keep trunks off walls and cull leaves inside
//!   buildings below the roof;
//! - no trunk stands within `blocks` blocks (Chebyshev: sideways or diagonally) of a building's
//!   footprint or a sealed surface (roads and paving): with 1, there is always exactly one free
//!   block between a tree and a building or road. A schematic tree is checked on every column of
//!   its trunk's base (big trees stand on 2 x 2 or 3 x 3 trunks), not only its centre;
//! - bushes, which spread a few blocks and know nothing of buildings, grow on no building's
//!   footprint (a bush on a wall line left a hole in the wall, its leaves in place of bricks).

use crate::bresenham::bresenham_line;
use crate::floodfill_cache::BuildingFootprintBitmap;
use crate::osm_parser::{ProcessedElement, ProcessedWay};
use crate::trees::schematic::{rotate_xz, Schematic};
use crate::world_editor::WorldEditor;
use std::sync::OnceLock;

/// Widest clearance accepted, in blocks.
pub const MAX_CLEARANCE: i32 = 8;

static CLEARANCE: OnceLock<i32> = OnceLock::new();
/// The footprints with their outlines, kept for the bushes (which are not handed them).
static FOOTPRINTS: OnceLock<BuildingFootprintBitmap> = OnceLock::new();

pub fn set(blocks: i32) -> Result<(), String> {
    if !(0..=MAX_CLEARANCE).contains(&blocks) {
        return Err(format!(
            "--tree-clearance {blocks}: must be 0 to {MAX_CLEARANCE} blocks"
        ));
    }
    CLEARANCE
        .set(blocks)
        .map_err(|_| "--tree-clearance set twice".to_string())
}

fn is_building(tags: &std::collections::HashMap<String, String>) -> bool {
    (tags.contains_key("building") || tags.contains_key("building:part"))
        && !crate::element_processing::buildings::is_underground_building(tags)
}

fn mark_outline(footprints: &mut BuildingFootprintBitmap, way: &ProcessedWay) {
    for pair in way.nodes.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for (x, _, z) in bresenham_line(a.x, 0, a.z, b.x, 0, b.z) {
            footprints.set(x, z);
        }
    }
}

/// Adds every building's outline to the footprint bitmap (with the flag only).
pub fn add_outlines(footprints: &mut BuildingFootprintBitmap, elements: &[ProcessedElement]) {
    if CLEARANCE.get().is_none() {
        return;
    }
    for element in elements {
        match element {
            ProcessedElement::Way(way) if is_building(&way.tags) => mark_outline(footprints, way),
            ProcessedElement::Relation(rel)
                if is_building(&rel.tags)
                    || rel.tags.get("type").map(String::as_str) == Some("building") =>
            {
                for member in &rel.members {
                    mark_outline(footprints, &member.way);
                }
            }
            _ => {}
        }
    }
    let _ = FOOTPRINTS.set(footprints.clone());
}

/// Whether (x, z) is on a building's footprint, outline included (with the flag only).
pub fn in_building(x: i32, z: i32) -> bool {
    FOOTPRINTS.get().is_some_and(|f| f.contains(x, z))
}

/// How far above a schematic's lowest log its trunk base reaches (as Arnis's root pass counts it).
const TRUNK_BASE: i32 = 2;

/// Whether any column of a schematic tree's trunk base, stamped at (anchor_x, anchor_z) turned by
/// `rot`, would stand within the clearance of a building or a road.
pub fn schematic_too_close(
    editor: &WorldEditor,
    schem: &Schematic,
    anchor_x: i32,
    anchor_z: i32,
    rot: u8,
    footprints: Option<&BuildingFootprintBitmap>,
) -> bool {
    if CLEARANCE.get().is_none() {
        return false;
    }
    let (fw, fl) = if rot & 1 == 0 {
        (schem.width, schem.length)
    } else {
        (schem.length, schem.width)
    };
    let (cx, cz) = ((fw - 1) / 2, (fl - 1) / 2);
    schem.voxels.iter().any(|&(vx, vy, vz, block)| {
        if i32::from(vy) > schem.min_log_vy + TRUNK_BASE
            || !crate::ground_decoration::WOOD.contains(&block)
        {
            return false;
        }
        let (rx, rz) = rotate_xz(i32::from(vx), i32::from(vz), schem.width, schem.length, rot);
        too_close(editor, footprints, anchor_x + rx - cx, anchor_z + rz - cz)
    })
}

/// Whether a trunk at (x, z) would stand within the clearance of a building or a road.
pub fn too_close(
    editor: &WorldEditor,
    footprints: Option<&BuildingFootprintBitmap>,
    x: i32,
    z: i32,
) -> bool {
    let Some(&r) = CLEARANCE.get() else {
        return false;
    };
    for dz in -r..=r {
        for dx in -r..=r {
            let (cx, cz) = (x + dx, z + dz);
            if footprints.is_some_and(|f| f.contains(cx, cz)) || editor.surface_is_sealed(cx, cz) {
                return true;
            }
        }
    }
    false
}
