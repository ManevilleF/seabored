#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = core::hint::black_box(seabored::serde::from_slice::<seabored::Value>(
        core::hint::black_box(data),
    ));
});
