//! Fuzz the untrusted-input path of `.spankpack` import (ADR-0009).
//!
//!   cargo install cargo-fuzz
//!   cd crates/spank-pack && cargo +nightly fuzz run read_pack
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    spank_pack::fuzz_read(data);
});
