//! Mineworld: inputs for invented worlds that replace Arnis's real-world data sources.
//!
//! Everything here is opt-in through its own flag; without one, Arnis behaves exactly as
//! upstream. The hooks into the rest of Arnis are kept to a line or two each, so upstream
//! merges stay easy.

pub mod climate;
pub mod heightmap;
pub mod land_cover;
pub mod snow;

use crate::args::Args;

/// Loads every mineworld input the arguments ask for. Call once, after argument validation.
pub fn init(args: &Args) -> Result<(), String> {
    if let Some(path) = &args.landcover {
        land_cover::load(path)?;
    }
    if let Some(path) = &args.heightmap {
        heightmap::load(path)?;
    }
    if let Some(path) = &args.climate {
        climate::load(path)?;
    }
    if let Some(y) = args.snow_line {
        snow::set(y)?;
    }
    Ok(())
}
