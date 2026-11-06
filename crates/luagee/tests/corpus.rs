//! The interpreter against its two corpora, end to end through the public API.
//!
//! `conformance/`: scripts with the output LuaJIT 2.1 prints for them (recorded with `tools/lua_diff`), which the
//! interpreter must print byte for byte. `hostile/`: scripts that attack the sandbox, each declaring on its first line
//! how it must end (`-- expect: limit NAME`, `-- expect: error TEXT`, `-- expect: ok [OUTPUT]`). What each kind
//! promises is in `support/corpus.rs`, which the wasm runner (`examples/corpus.rs`) shares.

#[allow(dead_code, reason = "the wasm runner uses the rest")]
#[path = "support/corpus.rs"]
mod corpus;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use corpus::Kind;
use luagee::{FolderLoader, GcStress};

/// The Rust stack an interpreter runs on, as a host should give it.
const STACK: usize = 16 << 20;
/// A hang guard only: no script comes near it unless a bound is broken.
const DEADLINE: Duration = Duration::from_secs(120);

fn folder(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

/// Runs every script of a corpus, each on a thread of its own with this native stack, and fails naming each one that
/// ended otherwise than its kind promises.
fn corpus(kind: Kind, name: &str, stack: usize, stress: GcStress) {
    let folder = folder(name);
    let module_size = kind.limits().module_size as u64;
    let mut failures = Vec::new();
    for script in kind.scripts(&folder) {
        let loader = FolderLoader::new(&folder, module_size).expect("the corpus folder");
        let (done, outcome) = mpsc::channel();
        let path = script.clone();
        std::thread::Builder::new()
            .name(script.display().to_string())
            .stack_size(stack)
            .spawn(move || {
                done.send(kind.check(&path, stress, loader))
                    .expect("the test waits for the result")
            })
            .expect("a script thread");
        let checked = outcome
            .recv_timeout(DEADLINE)
            .unwrap_or_else(|_| panic!("{} did not end within the hang guard", script.display()));
        failures.extend(checked.err());
    }
    assert!(
        failures.is_empty(),
        "{} {name} scripts ended otherwise:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn conformance_scripts_print_what_luajit_prints() {
    corpus(Kind::Conformance, "conformance", STACK, GcStress::Off);
}

/// The collector misses no root: a whole collection at every point changes no output.
#[test]
fn conformance_scripts_print_the_same_when_a_whole_collection_runs_at_every_instruction() {
    corpus(Kind::Conformance, "conformance", STACK, GcStress::Full);
}

/// The collector misses no barrier and frees nothing reached again: with an incremental collection always under
/// way, a few steps at every instruction, no output changes.
#[test]
fn conformance_scripts_print_the_same_when_a_collection_is_always_under_way() {
    corpus(
        Kind::Conformance,
        "conformance",
        STACK,
        GcStress::Incremental,
    );
}

#[test]
fn hostile_scripts_end_as_the_sandbox_promises() {
    corpus(Kind::Hostile, "hostile", STACK, GcStress::Off);
}

/// A host on a small stack (wasm's is 1 MiB) runs the corpora as well.
#[test]
fn the_corpora_pass_on_a_one_megabyte_native_stack() {
    corpus(Kind::Conformance, "conformance", 1 << 20, GcStress::Off);
    corpus(Kind::Hostile, "hostile", 1 << 20, GcStress::Off);
}
