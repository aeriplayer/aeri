//! Arbitrary bytes through probing and decoding must never panic, hang, or exhaust memory.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(mut decoder) = aeri_decode::open_bytes(data.to_vec()) {
        while let Ok(Some(_)) = decoder.next_block() {}
    }
});
