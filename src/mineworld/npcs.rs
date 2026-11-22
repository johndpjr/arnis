//! `--npcs <json>`: mobs placed by the front end (villagers, animals, golems, monsters), written
//! into the world as entities, where each one stands.
//!
//! The JSON is a list of `{ id, kind, x, z, name?, profession?, look? }` in world blocks (x east,
//! z south). The front end has already decided what may be written: hostile kinds only reach
//! here for a world that starts on Normal, and a villager's look is resolved from the land.
//!
//! - Each mob stands on the floor at its block: the first spot, going up from the ground, with
//!   something solid underfoot and room for its height above. Inside a building that is the
//!   ground floor, not the roof. A spot inside a wall (no floor near the ground) moves to the
//!   nearest block within two that has one, before giving up and taking the first floor above.
//! - Every mob is persistent, so the game never despawns it. A villager with a profession gets
//!   one point of trade experience: without a workstation, a villager with none loses its job
//!   (measured on the 26.3 server). A wandering trader stays (no despawn delay). A piglin is
//!   kept from turning into a zombified piglin in the Overworld.
//! - Entities are written in the template's 1.21.4 form (a name is a JSON text component in a
//!   string); the game upgrades them with the world.
//!
//! Like the rivers, a world built in tiles places each tile's mobs in that tile, before its
//! regions can be streamed to disk; a world built in one pass places them all after the merge.

use crate::world_editor::WorldEditor;
use fastnbt::Value;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Npc {
    pub id: String,
    pub kind: String,
    pub x: i32,
    pub z: i32,
    pub name: Option<String>,
    pub profession: Option<String>,
    pub look: Option<String>,
}

static NPCS: OnceLock<Vec<Npc>> = OnceLock::new();
static PLACED: AtomicUsize = AtomicUsize::new(0);
/// Mobs that found no floor near the ground (a spot inside a solid block or wall) and stand on
/// the first floor above, often a roof.
static LIFTED: AtomicUsize = AtomicUsize::new(0);

/// How far above the ground a floor counts as the ground floor (a raised foundation, a step).
const LOW: i32 = 6;
/// How far up to look for any floor at all.
const HIGH: i32 = 48;
/// How far sideways a spot in a wall may move to find a floor.
const NUDGE: i32 = 2;

pub fn load(path: &Path) -> Result<(), String> {
    let err = |e: String| format!("--npcs {}: {e}", path.display());
    let text = std::fs::read_to_string(path).map_err(|e| err(e.to_string()))?;
    let list: Vec<Npc> = serde_json::from_str(&text).map_err(|e| err(e.to_string()))?;
    for n in &list {
        if !KINDS.contains(&n.kind.as_str()) {
            return Err(err(format!("{}: unknown kind \"{}\"", n.id, n.kind)));
        }
    }
    NPCS.set(list).map_err(|_| err("loaded twice".to_string()))
}

/// Every kind the front end may send (schema/npcs.ts).
const KINDS: &[&str] = &[
    "villager",
    "wandering_trader",
    "iron_golem",
    "allay",
    "piglin",
    "cow",
    "pig",
    "sheep",
    "chicken",
    "vindicator",
    "pillager",
    "evoker",
    "witch",
    "zombie",
    "husk",
    "skeleton",
    "stray",
    "drowned",
    "piglin_brute",
    "zombified_piglin",
];

/// Blocks of headroom a kind needs.
fn height(kind: &str) -> i32 {
    if kind == "iron_golem" {
        3
    } else {
        2
    }
}

/// A stable hash of the NPC's id: seeds its UUID and the way it faces.
fn seed(id: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h as i64
}

/// The entity's own tags, beyond what every entity gets (position, UUID, motion).
fn tags(n: &Npc) -> HashMap<String, Value> {
    let mut t = HashMap::new();
    t.insert("PersistenceRequired".to_string(), Value::Byte(1));
    let yaw = (seed(&n.id).rem_euclid(360)) as f32 - 180.0;
    t.insert(
        "Rotation".to_string(),
        Value::List(vec![Value::Float(yaw), Value::Float(0.0)]),
    );
    if let Some(name) = n.name.as_deref().filter(|s| !s.trim().is_empty()) {
        t.insert(
            "CustomName".to_string(),
            Value::String(serde_json::json!({ "text": name }).to_string()),
        );
    }
    match n.kind.as_str() {
        "villager" => {
            let profession = n.profession.as_deref().unwrap_or("none");
            let mut data = HashMap::new();
            data.insert(
                "profession".to_string(),
                Value::String(format!("minecraft:{profession}")),
            );
            data.insert(
                "type".to_string(),
                Value::String(format!(
                    "minecraft:{}",
                    n.look.as_deref().unwrap_or("plains")
                )),
            );
            data.insert("level".to_string(), Value::Int(1));
            t.insert("VillagerData".to_string(), Value::Compound(data));
            if !matches!(profession, "none" | "nitwit") {
                t.insert("Xp".to_string(), Value::Int(1));
            }
        }
        "wandering_trader" => {
            t.insert("DespawnDelay".to_string(), Value::Int(0));
        }
        "piglin" => {
            t.insert("IsImmuneToZombification".to_string(), Value::Byte(1));
        }
        _ => {}
    }
    t
}

/// Where a mob of `need` blocks' height stands at (x, z): its block and the Y of its feet, and
/// whether it had to go higher than the ground floor. `open` says whether a cell has room for a
/// body (air, plants); `ground` is the terrain's surface Y.
fn stand(
    open: impl Fn(i32, i32, i32) -> bool,
    ground: impl Fn(i32, i32) -> i32,
    x: i32,
    z: i32,
    need: i32,
) -> (i32, i32, i32, bool) {
    let fits = |x: i32, y: i32, z: i32| !open(x, y - 1, z) && (0..need).all(|k| open(x, y + k, z));
    // The ground floor: the first open cell over something solid, near the ground. It must have
    // room for the mob; a room too low is not passed over for the roof above it.
    let low = |x: i32, z: i32| {
        let g = ground(x, z);
        (g + 1..=g + LOW)
            .find(|&y| open(x, y, z) && !open(x, y - 1, z))
            .filter(|&y| fits(x, y, z))
    };
    if let Some(y) = low(x, z) {
        return (x, y, z, false);
    }
    // Nearest first: rings of growing distance, each in a fixed order.
    for r in 1..=NUDGE {
        let mut ring: Vec<(i32, i32)> = (-r..=r)
            .flat_map(|dz| (-r..=r).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| dx.abs().max(dz.abs()) == r)
            .collect();
        ring.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
        for (dx, dz) in ring {
            if let Some(y) = low(x + dx, z + dz) {
                return (x + dx, y, z + dz, false);
            }
        }
    }
    let g = ground(x, z);
    match (g + LOW + 1..=g + HIGH).find(|&y| fits(x, y, z)) {
        Some(y) => (x, y, z, true),
        None => (x, g + 1, z, true),
    }
}

/// Places the NPCs standing in x min_x..=max_x, z min_z..=max_z: a tile's own area, after its
/// buildings, ground and water.
pub fn place_area(editor: &mut WorldEditor, min_x: i32, max_x: i32, min_z: i32, max_z: i32) {
    let Some(list) = NPCS.get() else {
        return;
    };
    let (mut placed, mut lifted) = (0, 0);
    for n in list {
        if n.x < min_x || n.x > max_x || n.z < min_z || n.z > max_z {
            continue;
        }
        let (x, y, z, high) = stand(
            |x, y, z| editor.cell_open_at(x, y, z),
            |x, z| editor.get_ground_level(x, z),
            n.x,
            n.z,
            height(&n.kind),
        );
        let id = format!("minecraft:{}", n.kind);
        if editor.add_entity_exact(
            &id,
            x as f64 + 0.5,
            y as f64,
            z as f64 + 0.5,
            seed(&n.id),
            tags(n),
        ) {
            placed += 1;
            if high {
                lifted += 1;
            }
        }
    }
    PLACED.fetch_add(placed, Ordering::Relaxed);
    LIFTED.fetch_add(lifted, Ordering::Relaxed);
}

/// After the merge: places them all when the ground was built on the merged editor; otherwise
/// each tile placed its own (`place_area`), and only the totals are reported.
pub fn place_merged(editor: &mut WorldEditor, ground_on_merged: bool) {
    if NPCS.get().is_none() {
        return;
    }
    if ground_on_merged {
        place_area(editor, i32::MIN, i32::MAX, i32::MIN, i32::MAX);
    }
    let placed = PLACED.swap(0, Ordering::Relaxed);
    let lifted = LIFTED.swap(0, Ordering::Relaxed);
    let total = NPCS.get().map_or(0, Vec::len);
    println!("NPCs: placed {placed} of {total}");
    if lifted > 0 {
        println!("NPCs: {lifted} found no floor near the ground (inside a wall or a solid block) and stand higher, maybe on a roof");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npc(kind: &str) -> Npc {
        Npc {
            id: "n1".to_string(),
            kind: kind.to_string(),
            x: 0,
            z: 0,
            name: None,
            profession: None,
            look: None,
        }
    }

    /// A world described by its solid cells; everything else is open.
    fn solid(cells: &[(i32, i32, i32)]) -> impl Fn(i32, i32, i32) -> bool + '_ {
        move |x, y, z| !cells.contains(&(x, y, z))
    }

    #[test]
    fn stands_on_the_ground_in_the_open() {
        let ground = [(0, 64, 0)];
        assert_eq!(stand(solid(&ground), |_, _| 64, 0, 0, 2), (0, 65, 0, false));
    }

    #[test]
    fn stands_on_the_ground_floor_inside_a_building_not_the_roof() {
        // Ground at 64, a floor block at 65, open rooms 66..=69, a roof at 70.
        let cells = [(0, 64, 0), (0, 65, 0), (0, 70, 0)];
        assert_eq!(stand(solid(&cells), |_, _| 64, 0, 0, 2), (0, 66, 0, false));
    }

    #[test]
    fn a_spot_in_a_wall_moves_to_the_nearest_floor_beside_it() {
        // A wall at x 0 from 64 to 70; open ground at x 1 and x -1.
        let mut cells: Vec<(i32, i32, i32)> = (64..=70).map(|y| (0, y, 0)).collect();
        for x in -3..=3 {
            for z in -3..=3 {
                if (x, z) != (0, 0) {
                    cells.push((x, 64, z));
                }
            }
        }
        let (x, y, z, high) = stand(solid(&cells), |_, _| 64, 0, 0, 2);
        assert_eq!((y, high), (65, false));
        assert_eq!(x.abs() + z.abs(), 1, "one block over, not diagonally");
    }

    #[test]
    fn a_golem_too_tall_for_a_room_steps_outside_rather_than_onto_the_roof() {
        // Ground 64 everywhere; at x 0 a low ceiling at 67: room for a villager only.
        let mut cells = vec![(0, 67, 0)];
        for x in -3..=3 {
            for z in -3..=3 {
                cells.push((x, 64, z));
            }
        }
        assert_eq!(stand(solid(&cells), |_, _| 64, 0, 0, 2), (0, 65, 0, false));
        let (x, y, z, high) = stand(solid(&cells), |_, _| 64, 0, 0, 3);
        assert_eq!((y, high), (65, false), "on the ground beside it");
        assert_eq!(x.abs() + z.abs(), 1);
    }

    #[test]
    fn tags_make_mobs_persistent_and_keep_jobs() {
        let mut v = npc("villager");
        v.profession = Some("farmer".to_string());
        v.look = Some("desert".to_string());
        v.name = Some("Hana \"the\" Baker".to_string());
        let t = tags(&v);
        assert_eq!(t.get("PersistenceRequired"), Some(&Value::Byte(1)));
        assert_eq!(
            t.get("Xp"),
            Some(&Value::Int(1)),
            "a job needs experience to last"
        );
        let Some(Value::Compound(data)) = t.get("VillagerData") else {
            panic!("no VillagerData");
        };
        assert_eq!(
            data.get("profession"),
            Some(&Value::String("minecraft:farmer".into()))
        );
        assert_eq!(
            data.get("type"),
            Some(&Value::String("minecraft:desert".into()))
        );
        let Some(Value::String(name)) = t.get("CustomName") else {
            panic!("no name");
        };
        let parsed: serde_json::Value = serde_json::from_str(name).unwrap();
        assert_eq!(
            parsed["text"], "Hana \"the\" Baker",
            "quotes survive as JSON"
        );

        let jobless = tags(&npc("villager"));
        assert!(!jobless.contains_key("Xp") && !jobless.contains_key("CustomName"));
        assert_eq!(
            tags(&npc("wandering_trader")).get("DespawnDelay"),
            Some(&Value::Int(0))
        );
        assert_eq!(
            tags(&npc("piglin")).get("IsImmuneToZombification"),
            Some(&Value::Byte(1))
        );
        assert!(!tags(&npc("cow")).contains_key("VillagerData"));
    }
}
