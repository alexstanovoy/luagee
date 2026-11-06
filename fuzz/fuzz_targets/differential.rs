//! Programs in the deterministic dialect, run by luagee and by a reference interpreter (LuaJIT 2.1 with its JIT off,
//! built by `tools/ref/build.sh`; `LUAGEE_REFERENCE` names it): both print the same. A run that ends at one of
//! luagee's limits is not compared (the reference has none); every other difference is a finding, unless
//! `known_difference` names it.
#![no_main]

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use luagee_fuzz::generate::{Dialect, program};
use luagee_fuzz::{KEYS, run};

static REFERENCE: LazyLock<String> = LazyLock::new(|| {
    std::env::var("LUAGEE_REFERENCE")
        .expect("LUAGEE_REFERENCE names the reference interpreter (tools/ref/build.sh)")
});

/// A generated program ends in milliseconds; one that takes this long is a generator bug.
const REFERENCE_TIMEOUT: Duration = Duration::from_secs(5);

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(source) = program(&mut u, Dialect::Deterministic) else {
        return;
    };
    let ours = run(source.as_bytes(), KEYS);
    if ours.ended.starts_with("Limit(") {
        return;
    }
    let (theirs, succeeded) = reference(&source);
    let ours_succeeded = ours.ended.starts_with("ok");
    if ours.printed == theirs && ours_succeeded == succeeded {
        return;
    }
    if let Some(reason) = known_difference(&ours.printed, &theirs) {
        let _ = reason;
        return;
    }
    panic!(
        "luagee and the reference differ\n--- source\n{source}\n--- luagee ({})\n{}\n--- reference (succeeded: {succeeded})\n{}",
        ours.ended,
        String::from_utf8_lossy(&ours.printed),
        String::from_utf8_lossy(&theirs)
    );
});

/// What the reference printed, and whether it ran to the end.
fn reference(source: &str) -> (Vec<u8>, bool) {
    let mut child = Command::new(&*REFERENCE)
        .args(["-joff", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the reference interpreter starts");
    child
        .stdin
        .take()
        .expect("a pipe")
        .write_all(source.as_bytes())
        .expect("the reference reads its program");
    let began = Instant::now();
    loop {
        if child.try_wait().expect("the reference's status").is_some() {
            break;
        }
        assert!(
            began.elapsed() < REFERENCE_TIMEOUT,
            "the reference ran past {REFERENCE_TIMEOUT:?}:\n{source}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = child.wait_with_output().expect("the reference's output");
    (output.stdout, output.status.success())
}

/// The differences that are luagee's documented choices (README's "Known differences"), by name.
fn known_difference(_ours: &[u8], _theirs: &[u8]) -> Option<&'static str> {
    None
}
