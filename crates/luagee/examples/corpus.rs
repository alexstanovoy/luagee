//! Runs every script of a corpus folder on the main thread, as `tests/corpus.rs` does on threads of its own, and
//! fails naming each script that ended otherwise than its kind promises; or the borrowed suites, as `tests/suites.rs`
//! runs them. It is how they run under wasm (one thread, a small stack): `tools/wasm.sh corpus` runs it under
//! wasmtime.
//!
//!     corpus KIND FOLDER...     KIND: conformance, hostile or fuzz (see tests/support/corpus.rs)
//!     corpus suites             the suites from /suites (panics naming what failed)

#[path = "../tests/support/corpus.rs"]
mod corpus;
#[path = "../tests/host_api.rs"]
mod host_api;
#[path = "../tests/host_api_safety.rs"]
mod host_api_safety;
#[path = "../tests/suites.rs"]
mod suites;

use std::path::Path;
use std::process::ExitCode;

use corpus::Kind;
use luagee::{FolderLoader, GcStress};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let usage = "usage: corpus KIND FOLDER... | corpus suites";
    if std::env::args().nth(1).as_deref() == Some("suites") {
        suites::puc_rio();
        suites::luau();
        suites::luajit();
        return ExitCode::SUCCESS;
    }
    if std::env::args().nth(1).as_deref() == Some("host-api") {
        host_api::contracts();
        host_api_safety::contracts();
        return ExitCode::SUCCESS;
    }
    let kind: Kind = match args.next().map(|k| k.parse()) {
        Some(Ok(kind)) => kind,
        Some(Err(error)) => {
            eprintln!("corpus: {error}\n{usage}");
            return ExitCode::FAILURE;
        }
        None => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    let folders: Vec<String> = args.collect();
    if folders.is_empty() {
        eprintln!("{usage}");
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for folder in &folders {
        let folder = Path::new(folder);
        let scripts = kind.scripts(folder);
        for script in &scripts {
            let loader = FolderLoader::new(folder, kind.limits().module_size as u64)
                .expect("the corpus folder");
            if let Err(what) = kind.check(script, GcStress::Off, loader) {
                eprintln!("FAIL {what}");
                failed += 1;
            }
        }
        println!(
            "corpus: {} {kind:?} scripts in {}, {failed} failed so far",
            scripts.len(),
            folder.display()
        );
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
