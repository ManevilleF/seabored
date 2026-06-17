#![no_main]

use libfuzzer_sys::fuzz_target;
use seabored::de::CborDeserialize as _;

fuzz_target!(|data: &[u8]| {
    let _ = core::hint::black_box(seabored::Value::cbor_deserialize_from(
        core::hint::black_box(&mut &data[..]),
    ));
});
