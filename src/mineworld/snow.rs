//! `--snow-line <Y>`: the snow line at a chosen Y instead of the climatic one.
//!
//! Arnis places its snow line from latitude (about 3000 m at 45 degrees), with a 200 m band
//! from the first snow to full cover and an alpine band of meadow and stony peaks 1000 m below
//! it. An invented world's mountains are a few hundred blocks tall, so with the flag the line
//! sits at the given Y and both bands are sized for such mountains. Arnis's snow model itself
//! is unchanged: flat ground and hollows hold snow, steep faces and ridges shed it, slopes
//! facing away from the sun keep it lower. Needs `--heightmap`, whose mapping is one block per
//! metre, so these block sizes are metres too.

use std::sync::OnceLock;

/// Blocks from the snow line to where snow fully covers flat ground (it does at 0.6 bands).
pub const BAND_BLOCKS: f64 = 20.0;
/// Blocks of alpine band (meadow, stony peaks, alpine plants) below the snow line.
pub const ALPINE_BAND_BLOCKS: f64 = 40.0;

static LINE: OnceLock<i32> = OnceLock::new();

pub fn set(y: i32) -> Result<(), String> {
    LINE.set(y).map_err(|_| "--snow-line set twice".to_string())
}

/// The chosen snow line, if any.
pub fn line() -> Option<i32> {
    LINE.get().copied()
}

/// Snow band in blocks: the chosen line's, else `default`.
pub fn band_blocks(default: f64) -> f64 {
    band_for(line(), default)
}

/// Alpine band in metres (= blocks under `--heightmap`): the chosen line's, else `default`.
pub fn alpine_band_metres(default: f64) -> f64 {
    alpine_for(line(), default)
}

fn band_for(line: Option<i32>, default: f64) -> f64 {
    line.map_or(default, |_| BAND_BLOCKS)
}

fn alpine_for(line: Option<i32>, default: f64) -> f64 {
    line.map_or(default, |_| ALPINE_BAND_BLOCKS)
}

#[cfg(test)]
mod tests {
    use super::*;

    // `LINE` is process-wide and other tests expect the climatic snow line, so these test the
    // pure parts and never call `set`.
    #[test]
    fn bands_are_sized_for_painted_mountains_only_with_the_flag() {
        assert_eq!(band_for(None, 200.0), 200.0);
        assert_eq!(alpine_for(None, 1000.0), 1000.0);
        assert_eq!(band_for(Some(160), 200.0), BAND_BLOCKS);
        assert_eq!(alpine_for(Some(160), 1000.0), ALPINE_BAND_BLOCKS);
    }
}
