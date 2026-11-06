//! The interpreter against its two corpora, end to end through the public API.
//!
//! `conformance/`: scripts with the output LuaJIT 2.1 prints for them (recorded with `tools/lua_diff`), which the
//! interpreter must print byte for byte. `hostile/`: scripts that attack the sandbox, each declaring on its first line
//! how it must end (`-- expect: limit NAME`, `-- expect: error TEXT`, `-- expect: ok [OUTPUT]`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use luagee::{Error, FolderLoader, Limit, Limits, Lua};

/// The Rust stack an interpreter runs on, as a host should give it.
const STACK: usize = 16 << 20;
/// A hang guard only: no script comes near it unless a bound is broken.
const DEADLINE: Duration = Duration::from_secs(120);

fn scripts(folder: &str) -> Vec<PathBuf> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(folder);
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&folder)
        .expect("the corpus folder")
        .map(|entry| entry.expect("a corpus entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "lua"))
        .collect();
    scripts.sort();
    scripts
}

/// Runs a script on a thread of its own with these limits: what it printed, and how it ended.
fn run(script: &Path, limits: Limits) -> (Vec<u8>, Result<(), Error>) {
    let script = script.to_path_buf();
    let (done, outcome) = mpsc::channel();
    let name = script
        .file_name()
        .expect("a file name")
        .to_string_lossy()
        .into_owned();
    let chunk = name.clone();
    std::thread::Builder::new()
        .name(name.clone())
        .stack_size(STACK)
        .spawn(move || {
            let source = std::fs::read(&script).expect("read the script");
            let folder = script.parent().expect("a folder").to_path_buf();
            let printed = Rc::new(RefCell::new(Vec::new()));
            let mut lua = Lua::new(limits, 7).expect("an interpreter");
            let sink = printed.clone();
            lua.set_print(Box::new(move |line| {
                sink.borrow_mut().extend_from_slice(line)
            }));
            lua.set_loader(
                FolderLoader::new(&folder, limits.module_size as u64).expect("the corpus folder"),
            );
            let ended = lua.exec(&chunk, &source).map(|_| ());
            let printed = printed.borrow().clone();
            done.send((printed, ended))
                .expect("the test waits for the result");
        })
        .expect("a script thread");
    outcome
        .recv_timeout(DEADLINE)
        .unwrap_or_else(|_| panic!("{name} did not end within the hang guard"))
}

#[test]
fn conformance_scripts_print_what_luajit_prints() {
    let limits = Limits {
        fuel_per_call: 1 << 36,
        fuel_per_tick: 1 << 36,
        ..Limits::DEFAULT
    };
    let mut failures = Vec::new();
    for script in scripts("conformance") {
        let expected = std::fs::read(script.with_extension("out")).expect("the recorded output");
        let (printed, ended) = run(&script, limits);
        if let Err(error) = ended {
            failures.push(format!("{}: {error}", script.display()));
        } else if printed != expected {
            let line = printed
                .split(|&c| c == b'\n')
                .zip(expected.split(|&c| c == b'\n'))
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            failures.push(format!(
                "{}: output differs from line {}",
                script.display(),
                line + 1
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} scripts differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

enum Expectation {
    Limit(Limit),
    Error(String),
    Ok(String),
}

fn expectation(script: &Path) -> Expectation {
    let source = std::fs::read_to_string(script).expect("a hostile script is text");
    let first = source.lines().next().unwrap_or_default();
    let rule = first
        .strip_prefix("-- expect: ")
        .unwrap_or_else(|| panic!("{} has no expect line", script.display()));
    if let Some(name) = rule.strip_prefix("limit ") {
        return Expectation::Limit(match name {
            "fuel_per_call" => Limit::FuelPerCall,
            "fuel_per_tick" => Limit::FuelPerTick,
            "memory" => Limit::Memory,
            "module_size" => Limit::ModuleSize,
            "scripts_size" => Limit::ScriptsSize,
            "module_count" => Limit::ModuleCount,
            other => panic!("{}: unknown limit {other}", script.display()),
        });
    }
    if let Some(text) = rule.strip_prefix("error ") {
        return Expectation::Error(text.to_owned());
    }
    match rule.strip_prefix("ok") {
        Some(text) => Expectation::Ok(text.strip_prefix(' ').unwrap_or(text).to_owned()),
        None => panic!("{}: unknown expectation {rule}", script.display()),
    }
}

#[test]
fn hostile_scripts_end_as_the_sandbox_promises() {
    let limits = Limits {
        fuel_per_call: 5_000_000,
        fuel_per_tick: 5_000_000,
        memory: 16 << 20,
        ..Limits::DEFAULT
    };
    let mut failures = Vec::new();
    for script in scripts("hostile") {
        let (printed, ended) = run(&script, limits);
        let printed = String::from_utf8_lossy(&printed);
        let printed = printed.strip_suffix('\n').unwrap_or(&printed);
        let fine = match (expectation(&script), &ended) {
            (Expectation::Limit(limit), Err(Error::Limit(got))) => limit == *got,
            (
                Expectation::Error(text),
                Err(Error::Runtime { message, .. } | Error::Syntax { message, .. }),
            ) => message.contains(&text),
            (Expectation::Ok(text), Ok(())) => printed == text,
            _ => false,
        };
        if !fine {
            let outcome = match &ended {
                Ok(()) => format!("ok {printed:?}"),
                Err(error) => format!("{error}"),
            };
            failures.push(format!(
                "{}: ended with {}",
                script.display(),
                outcome.lines().next().unwrap_or_default()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} hostile scripts ended otherwise:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
