//! Arbitrary text through the M3U parser must never panic, hang, or exhaust memory.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let sources = aeri_player::parse_m3u(&text, Path::new("/library"));
    assert!(sources.len() <= text.lines().count());
});
