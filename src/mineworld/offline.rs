//! `--offline`: a run that never touches the network.
//!
//! Invented worlds bring all their data (OSM file, painted heights, land cover, climate), so
//! nothing should be downloaded. With the flag:
//! - the startup update check (GitHub releases API) is skipped;
//! - the Mapillary facade pipeline stays off whatever MAPILLARY_TOKEN says;
//! - as a safety net for any download path not covered above (3D models from tags, a future
//!   upstream fetch), every HTTP client, including curl or wget child processes, is pointed at a
//!   dead local proxy, so a request fails at once instead of reaching the network.
//!
//! The safety net covers HTTP clients that honour the proxy variables (reqwest does by default).
//! mineworld's `just offline-check` traces a whole run for connection attempts of any kind.

use crate::args::Args;

pub const FLAG: &str = "--offline";

/// A proxy nobody listens on (the discard port on loopback): requests fail with "connection
/// refused" without leaving the machine.
const DEAD_PROXY: &str = "http://127.0.0.1:9";
const PROXY_VARS: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];
const NO_PROXY_VARS: [&str; 2] = ["NO_PROXY", "no_proxy"];

/// Whether the command line asks for an offline run. Read from the raw arguments, because the
/// update check starts before they are parsed.
pub fn requested() -> bool {
    requested_in(std::env::args())
}

fn requested_in(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().skip(1).any(|a| a == FLAG)
}

/// Points every HTTP client at the dead proxy. Call first thing in `main`, before any thread
/// starts (the environment is process-wide).
pub fn arm_network_guard() {
    for v in PROXY_VARS {
        std::env::set_var(v, DEAD_PROXY);
    }
    for v in NO_PROXY_VARS {
        std::env::remove_var(v);
    }
}

/// Turns off the features that download by default or from the environment.
pub fn apply(args: &mut Args) {
    if !args.offline {
        return;
    }
    args.mapillary_token = None;
    args.mapillary_facades = Some(false);
    args.mapillary_probe = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_found_in_the_raw_arguments() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(requested_in(argv(&[
            "arnis",
            "--file",
            "x.osm",
            "--offline"
        ])));
        assert!(!requested_in(argv(&["arnis", "--file", "x.osm"])));
        assert!(
            !requested_in(argv(&["--offline"])),
            "argv[0] is the program"
        );
    }
}
