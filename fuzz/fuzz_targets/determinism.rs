//! The same source and keys give the same run: what it prints, how it ends, the fuel it spends and the memory it
//! counts. A different hash key changes none of these either: iteration order and costs never depend on it, so a
//! script cannot learn the key from what it sees or from when it runs out of fuel.
#![no_main]

use libfuzzer_sys::fuzz_target;
use luagee::Keys;
use luagee_fuzz::run;

fuzz_target!(|data: &[u8]| {
    let keys = Keys {
        hash: [1, 2],
        random: 3,
    };
    let first = run(data, keys);
    let second = run(data, keys);
    assert_eq!(first, second, "the same source and keys ran differently");
    let rekeyed = run(
        data,
        Keys {
            hash: [0x9e37_79b9_7f4a_7c15, 7],
            ..keys
        },
    );
    assert_eq!(first, rekeyed, "another hash key changed the run");
});
