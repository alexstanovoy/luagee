//! What a corpus is and how its scripts must end, shared by the corpus tests (`tests/corpus.rs`, natively) and the
//! corpus runner (`examples/corpus.rs`, under wasm), so that both check the same promises.
//!
//! A corpus is a folder of scripts of one [`Kind`], which sets the limits they run under and how each must end.
//! Scripts run on the calling thread: the caller chooses the native stack.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[allow(
    clippy::duplicate_mod,
    reason = "the wasm runner also includes the external embedding test module"
)]
#[path = "host_api.rs"]
mod host_api;

use luagee::{Error, GcStress, Limit, Limits, Loader, Lua};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `NAME.lua` must print `NAME.out` byte for byte and end without an error.
    Conformance,
    /// `NAME.lua` declares on its first line how it must end: `-- expect: limit NAME`, `-- expect: error TEXT` (the
    /// message contains TEXT) or `-- expect: ok [OUTPUT]`.
    Hostile,
    /// Every file is a script from a stranger (a fuzzer's input, whatever target it came from): it may end in any
    /// way, but the interpreter must stay usable after it ([`survives`]). Under the fuzz targets' limits.
    Fuzz,
}

impl std::str::FromStr for Kind {
    type Err = String;

    fn from_str(name: &str) -> Result<Kind, String> {
        match name {
            "conformance" => Ok(Kind::Conformance),
            "hostile" => Ok(Kind::Hostile),
            "fuzz" => Ok(Kind::Fuzz),
            other => Err(format!(
                "unknown corpus kind {other:?} (conformance, hostile or fuzz)"
            )),
        }
    }
}

impl Kind {
    pub fn limits(self) -> Limits {
        match self {
            // The conformance scripts are long programs: fuel is no part of what they check.
            Kind::Conformance => Limits {
                fuel_per_call: 1 << 36,
                fuel_per_tick: 1 << 36,
                ..Limits::DEFAULT
            },
            Kind::Hostile => Limits {
                fuel_per_call: 5_000_000,
                fuel_per_tick: 5_000_000,
                memory: 16 << 20,
                ..Limits::DEFAULT
            },
            Kind::Fuzz => FUZZ_LIMITS,
        }
    }

    /// The scripts of a folder of this kind, sorted.
    pub fn scripts(self, folder: &Path) -> Vec<PathBuf> {
        let mut scripts: Vec<PathBuf> = std::fs::read_dir(folder)
            .unwrap_or_else(|e| panic!("the corpus folder {}: {e}", folder.display()))
            .map(|entry| entry.expect("a corpus entry").path())
            .filter(|path| match self {
                Kind::Conformance | Kind::Hostile => path.extension().is_some_and(|e| e == "lua"),
                Kind::Fuzz => path.is_file(),
            })
            .collect();
        scripts.sort();
        scripts
    }

    /// Runs a script of this kind on the calling thread and checks how it ended: Err tells what went otherwise.
    pub fn check(
        self,
        script: &Path,
        stress: GcStress,
        loader: impl Loader + 'static,
    ) -> Result<(), String> {
        let source = std::fs::read(script).map_err(|e| format!("{}: {e}", script.display()))?;
        let fail = |what: String| Err(format!("{}: {what}", script.display()));
        let mut lua = interpreter(self.limits(), stress, loader);
        if self == Kind::Fuzz {
            let host = script
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "host_api");
            if host {
                host_api::install(&mut lua.lua).map_err(|e| e.to_string())?;
            }
            return match survives(&mut lua.lua, &source, if host { 2 } else { 1 }) {
                Ok(()) => Ok(()),
                Err(what) => fail(what),
            };
        }
        let chunk = script.file_name().expect("a file name").to_string_lossy();
        let ended = lua.lua.exec(&chunk, &source).map(|_| ());
        let printed = lua.printed.borrow();
        match self {
            Kind::Conformance => {
                let expected = std::fs::read(script.with_extension("out"))
                    .map_err(|e| format!("{}: the recorded output: {e}", script.display()))?;
                match ended {
                    Err(error) => fail(format!("{error}")),
                    Ok(()) if *printed != expected => fail(format!(
                        "output differs from line {}",
                        first_difference(&printed, &expected)
                    )),
                    Ok(()) => Ok(()),
                }
            }
            Kind::Hostile => {
                let printed = String::from_utf8_lossy(&printed);
                let printed = printed.strip_suffix('\n').unwrap_or(&printed);
                let fine = match (expectation(script, &source)?, &ended) {
                    (Expectation::Limit(limit), Err(Error::Limit(got))) => limit == *got,
                    (
                        Expectation::Error(text),
                        Err(Error::Runtime { message, .. } | Error::Syntax { message, .. }),
                    ) => message.contains(&text),
                    (Expectation::Ok(text), Ok(())) => printed == text,
                    _ => false,
                };
                if fine {
                    return Ok(());
                }
                let outcome = match &ended {
                    Ok(()) => format!("ok {printed:?}"),
                    Err(error) => format!("{error}"),
                };
                fail(format!(
                    "ended with {}",
                    outcome.lines().next().unwrap_or_default()
                ))
            }
            Kind::Fuzz => unreachable!("handled above"),
        }
    }
}

/// Small limits, so each input ends fast: those the fuzz targets run their inputs under.
pub const FUZZ_LIMITS: Limits = Limits {
    fuel_per_call: 200_000,
    fuel_per_tick: 400_000,
    memory: 8 << 20,
    string_length: 1 << 20,
    call_depth: 400,
    syntax_depth: 100,
    pattern_depth: 100,
    function_upvalues: 60,
    module_size: 1 << 16,
    scripts_size: 1 << 18,
    module_count: 32,
};

struct Interpreter {
    lua: Lua,
    printed: Rc<RefCell<Vec<u8>>>,
}

fn interpreter(limits: Limits, stress: GcStress, loader: impl Loader + 'static) -> Interpreter {
    let printed = Rc::new(RefCell::new(Vec::new()));
    let mut lua = Lua::new(
        limits,
        luagee::Keys {
            hash: [7, 7],
            random: 7,
        },
    )
    .expect("an interpreter");
    let sink = printed.clone();
    lua.set_gc_stress(stress);
    lua.set_print(Box::new(move |line| {
        sink.borrow_mut().extend_from_slice(line)
    }));
    lua.set_loader(loader);
    Interpreter { lua, printed }
}

/// What the fuzz target checks of a run, but for the real memory it held (which needs the host's allocator): any
/// outcome of the input is fine; the interpreter's count of its memory stays near the cap; and the interpreter still
/// runs a function after it (a collection the script left may take a few ticks' fuel, and what the script left alive
/// may fill the cap, which is the cap working), and still collects.
pub fn survives(lua: &mut Lua, input: &[u8], repetitions: usize) -> Result<(), String> {
    // Loaded first: the input may use up the source limits, which last the interpreter's life.
    let after = lua
        .load(
            "after",
            b"local t = {} for i = 1, 10 do t[i] = i end return #t",
        )
        .map_err(|e| format!("the probe does not compile: {e}"))?;
    lua.pin(after).expect("root fits");
    for attempt in 0..repetitions {
        if attempt > 0 {
            lua.begin_tick();
        }
        let _ = lua.exec("fuzz", input);
    }
    let ceiling = FUZZ_LIMITS.memory + FUZZ_LIMITS.memory / 4;
    if lua.memory() > ceiling {
        return Err(format!(
            "counted {} bytes, past the ceiling {ceiling}",
            lua.memory()
        ));
    }
    let mut ran = Err(Error::Limit(Limit::FuelPerCall));
    for _ in 0..10 {
        lua.begin_tick();
        ran = lua.call(after, &[]);
        if !matches!(
            ran,
            Err(Error::Limit(Limit::FuelPerCall | Limit::FuelPerTick))
        ) {
            break;
        }
    }
    match ran {
        Ok(values) if values.len() == 1 => {}
        Err(Error::Limit(Limit::Memory)) => {}
        other => return Err(format!("the interpreter broke: {other:?}")),
    }
    match lua.collect() {
        Ok(()) if lua.memory() > FUZZ_LIMITS.memory => {
            Err(format!("a collection left {} bytes", lua.memory()))
        }
        Ok(()) | Err(Error::Limit(Limit::Memory)) => Ok(()),
        Err(error) => Err(format!("a collection failed: {error:?}")),
    }
}

fn first_difference(printed: &[u8], expected: &[u8]) -> usize {
    let line = printed
        .split(|&c| c == b'\n')
        .zip(expected.split(|&c| c == b'\n'))
        .position(|(a, b)| a != b)
        .unwrap_or(0);
    line + 1
}

enum Expectation {
    Limit(Limit),
    Error(String),
    Ok(String),
}

fn expectation(script: &Path, source: &[u8]) -> Result<Expectation, String> {
    let first = source.split(|&c| c == b'\n').next().unwrap_or_default();
    let first = String::from_utf8_lossy(first);
    let unknown = |what: &str| Err(format!("{}: {what}", script.display()));
    let Some(rule) = first.trim_end_matches('\r').strip_prefix("-- expect: ") else {
        return unknown("no expect line");
    };
    if let Some(name) = rule.strip_prefix("limit ") {
        return Ok(Expectation::Limit(match name {
            "fuel_per_call" => Limit::FuelPerCall,
            "fuel_per_tick" => Limit::FuelPerTick,
            "memory" => Limit::Memory,
            "module_size" => Limit::ModuleSize,
            "scripts_size" => Limit::ScriptsSize,
            "module_count" => Limit::ModuleCount,
            other => return unknown(&format!("unknown limit {other}")),
        }));
    }
    if let Some(text) = rule.strip_prefix("error ") {
        return Ok(Expectation::Error(text.to_owned()));
    }
    match rule.strip_prefix("ok") {
        Some(text) => Ok(Expectation::Ok(
            text.strip_prefix(' ').unwrap_or(text).to_owned(),
        )),
        None => unknown(&format!("unknown expectation {rule}")),
    }
}
