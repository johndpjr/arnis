//! `--climate <png>`: climate per block instead of one for the whole world.
//!
//! Arnis takes one climate for the whole area, from the Koppen grid at its centre, and decides
//! jungle by latitude. An invented world paints its regions: the PNG is 8-bit greyscale, one
//! pixel per world block (x east, z south, from the world's north-west corner), each value a
//! climate code below, 0 = the world's own climate. Everything Arnis derives from climate reads
//! it per block: surface blocks, biomes, ground plants, bushes, rocks, fields, building styles,
//! and the habitat of the trees (palms and jungle where tropical, conifers in boreal and tundra
//! country, dry woodland in savanna and steppe). The snow line stays separate (--snow-line).

use crate::climate::Climate;
use crate::trees::region::Habitat;
use std::path::Path;
use std::sync::OnceLock;

/// The world's own climate (Arnis's, from its location).
pub const DEFAULT: u8 = 0;
pub const TEMPERATE: u8 = 1;
/// Temperate rules, with jungle where forest grows and palms.
pub const TROPICAL: u8 = 2;
pub const SAVANNA: u8 = 3;
pub const HOT_DESERT: u8 = 4;
pub const HOT_STEPPE: u8 = 5;
pub const COLD_DESERT: u8 = 6;
pub const COLD_STEPPE: u8 = 7;
pub const BOREAL: u8 = 8;
pub const TUNDRA: u8 = 9;
pub const ICE_CAP: u8 = 10;
const MAX_CODE: u8 = ICE_CAP;

/// Latitude a tropical block counts as (jungle, warm sea), and the least a painted
/// non-tropical one does, so latitude rules agree with the painted climate. Polar climates
/// count as 65: below 60 Arnis reads tundra and ice cap as mountain tops in boreal country.
const TROPICAL_LAT: f64 = 10.0;
const EXTRATROPICAL_LAT: f64 = 30.0;
const POLAR_LAT: f64 = 65.0;

struct Painted {
    width: usize,
    height: usize,
    data: Vec<u8>,
    any_tropical: bool,
}

static PAINTED: OnceLock<Painted> = OnceLock::new();

pub fn load(path: &Path) -> Result<(), String> {
    let err = |e: String| format!("--climate {}: {e}", path.display());
    let img = image::ImageReader::open(path)
        .map_err(|e| err(e.to_string()))?
        .decode()
        .map_err(|e| err(e.to_string()))?;
    if img.color() != image::ColorType::L8 {
        return Err(err(format!(
            "must be an 8-bit greyscale PNG of climate codes (got {:?})",
            img.color()
        )));
    }
    let gray = img.into_luma8();
    let (width, height) = (gray.width() as usize, gray.height() as usize);
    let data = gray.into_raw();
    if let Some(&bad) = data.iter().find(|&&c| c > MAX_CODE) {
        return Err(err(format!(
            "climate code {bad} is not one of 0..={MAX_CODE}"
        )));
    }
    let any_tropical = data.contains(&TROPICAL);
    PAINTED
        .set(Painted {
            width,
            height,
            data,
            any_tropical,
        })
        .map_err(|_| err("loaded twice".to_string()))
}

/// The painted code at a world block (clamped to the map), or None without a climate map.
fn code(x: i32, z: i32) -> Option<u8> {
    let p = PAINTED.get()?;
    let cx = (x.max(0) as usize).min(p.width - 1);
    let cz = (z.max(0) as usize).min(p.height - 1);
    Some(p.data[cz * p.width + cx])
}

/// Arnis's climate for a code; `world` for the default.
fn climate_of(code: u8, world: Climate) -> Climate {
    match code {
        TEMPERATE | TROPICAL => Climate::Temperate,
        SAVANNA => Climate::TropicalSavanna,
        HOT_DESERT => Climate::HotDesert,
        HOT_STEPPE => Climate::HotSteppe,
        COLD_DESERT => Climate::ColdDesert,
        COLD_STEPPE => Climate::ColdSteppe,
        BOREAL => Climate::Boreal,
        TUNDRA => Climate::Tundra,
        ICE_CAP => Climate::IceCap,
        _ => world,
    }
}

/// Absolute latitude latitude rules should use for a code; `world` for the default.
fn abs_lat_of(code: u8, world: f64) -> f64 {
    match code {
        DEFAULT => world,
        TROPICAL => TROPICAL_LAT,
        TUNDRA | ICE_CAP => world.max(POLAR_LAT),
        _ => world.max(EXTRATROPICAL_LAT),
    }
}

/// Tree habitat a code asks for, if it asks for one.
fn habitat_of(code: u8) -> Option<Habitat> {
    match code {
        TROPICAL => Some(Habitat::Tropical),
        BOREAL | TUNDRA | ICE_CAP => Some(Habitat::Conifer),
        SAVANNA | HOT_DESERT | HOT_STEPPE | COLD_DESERT | COLD_STEPPE => Some(Habitat::Dry),
        _ => None,
    }
}

/// Climate at a world block; `world` (Arnis's climate for the area) where none is painted.
pub fn at(x: i32, z: i32, world: Climate) -> Climate {
    code(x, z).map_or(world, |c| climate_of(c, world))
}

/// Absolute latitude for latitude rules (jungle, warm sea) at a world block.
pub fn abs_lat(x: i32, z: i32, world: f64) -> f64 {
    code(x, z).map_or(world, |c| abs_lat_of(c, world))
}

/// Tree habitat the painted climate asks for at a world block.
pub fn habitat(x: i32, z: i32) -> Option<Habitat> {
    code(x, z).and_then(habitat_of)
}

/// Whether palms may grow at a world block: where the painted climate is tropical.
pub fn palms(x: i32, z: i32) -> bool {
    code(x, z) == Some(TROPICAL)
}

/// Whether any block is tropical (palms must then be loaded with the tree pack).
pub fn any_tropical() -> bool {
    PAINTED.get().is_some_and(|p| p.any_tropical)
}

#[cfg(test)]
mod tests {
    use super::*;

    // `PAINTED` is process-wide and other tests expect one climate, so these test the pure parts.
    #[test]
    fn codes_map_to_arnis_climates() {
        assert_eq!(climate_of(DEFAULT, Climate::Boreal), Climate::Boreal);
        assert_eq!(climate_of(TROPICAL, Climate::Boreal), Climate::Temperate);
        assert_eq!(
            climate_of(HOT_DESERT, Climate::Temperate),
            Climate::HotDesert
        );
        assert_eq!(
            climate_of(SAVANNA, Climate::Temperate),
            Climate::TropicalSavanna
        );
        assert_eq!(climate_of(ICE_CAP, Climate::Temperate), Climate::IceCap);
    }

    #[test]
    fn latitude_agrees_with_the_painted_climate() {
        assert_eq!(abs_lat_of(DEFAULT, 45.0), 45.0);
        assert!(abs_lat_of(TROPICAL, 45.0) < 23.5, "tropical grows jungle");
        assert!(
            abs_lat_of(TEMPERATE, 5.0) > 23.5,
            "painted temperate does not"
        );
        assert_eq!(abs_lat_of(BOREAL, 60.0), 60.0);
        assert!(abs_lat_of(TUNDRA, 45.0) >= 60.0, "tundra stays tundra");
    }

    #[test]
    fn habitats_follow_the_climate() {
        assert_eq!(habitat_of(TROPICAL), Some(Habitat::Tropical));
        assert_eq!(habitat_of(BOREAL), Some(Habitat::Conifer));
        assert_eq!(habitat_of(HOT_STEPPE), Some(Habitat::Dry));
        assert_eq!(habitat_of(TEMPERATE), None);
        assert_eq!(habitat_of(DEFAULT), None);
    }
}
