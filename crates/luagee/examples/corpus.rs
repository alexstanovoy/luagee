//! Runs every script of a corpus folder on the main thread, as `tests/corpus.rs` does on threads of its own, and
//! fails naming each script that ended otherwise than its kind promises. It is how the corpora run under wasm (one
//! thread, the default stack): `tools/wasm.sh corpus` runs it under wasmtime.
//!
//!     corpus KIND FOLDER...     KIND: conformance, hostile or fuzz (see tests/support/corpus.rs)

#[path = "../tests/support/corpus.rs"]
mod corpus;

use std::path::Path;
use std::process::ExitCode;

use corpus::Kind;
use luagee::{FolderLoader, GcStress};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let usage = "usage: corpus KIND FOLDER...";
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
            let loader = FolderLoader::new(folder, kind.limits().module_size as u64).expect("the corpus folder");
            if let Err(what) = kind.check(script, GcStress::Off, loader) {
                eprintln!("FAIL {what}");
                failed += 1;
            }
        }
        println!("corpus: {} {kind:?} scripts in {}, {failed} failed so far", scripts.len(), folder.display());
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
