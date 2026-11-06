//! Other implementations' test suites, end to end through the public API, under the sandbox's limits.
//!
//! `suites/` holds them as their authors published them (each folder's `PROVENANCE.md` names the source, the commit
//! and the checksums; the license sits next to it): the PUC-Rio Lua 5.1 tests, LuaJIT's test-cleanup tree, and the
//! Luau conformance scripts that are Lua 5.1. A Lua 5.1 or Luau file passes when it runs to its end as the suite's
//! own driver expects (Luau's return "OK"); a LuaJIT file holds cases (`do --- name`), each run as LuaJIT's
//! `test.lua` runs them.
//!
//! `suites/skips.txt` lists everything that does not run as written, each with its reason; nothing else is left out:
//! - `skip TARGET -- REASON`: the rest runs without the target, and the target itself must still fail (a lifted
//!   range must fail inside itself), so a skip that starts passing fails the test;
//! - `omit TARGET -- REASON`: never runs (the suite's own driver, a helper module, or a test of what the sandbox leaves
//!   out on purpose, which would pass vacuously or not at all).
//!
//! A target is `SUITE/FILE`, `SUITE/FILE:FROM-TO` (those lines blanked, so that every other line keeps its number) or
//! `SUITE/FILE#CASE`. An entry that matches nothing fails the test as well.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use luagee::{Error, FolderLoader, Keys, Limits, Lua, Value};

/// The Rust stack an interpreter runs on, as a host should give it.
const STACK: usize = 16 << 20;
/// A hang guard only: no file comes near it unless a bound is broken.
const DEADLINE: Duration = Duration::from_secs(120);
/// The interpreters one suite runs at once: the suites' tests run side by side, so a few each.
const WORKERS: usize = 4;

/// The default limits, with the fuel raised so that the longest file (string and sort stress) ends, and room for the
/// thousands of chunks the suites compile with `loadstring` in loops.
const LIMITS: Limits = Limits {
    fuel_per_call: 1 << 32,
    fuel_per_tick: 1 << 32,
    module_count: 1 << 16,
    ..Limits::DEFAULT
};

/// The suites' folder: next to this file, or `/suites` under wasm, where `tools/wasm.sh` gives it to the guest (the
/// folder loader resolves a folder only below a top-level guest folder there).
fn suites() -> PathBuf {
    if cfg!(target_os = "wasi") {
        return PathBuf::from("/suites");
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/suites")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Skip,
    Omit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    File,
    Lines(u32, u32),
    Case(String),
}

/// A line of `skips.txt`.
#[derive(Debug)]
struct Entry {
    line: usize,
    kind: Kind,
    file: String,
    part: Part,
    /// The reason's first word (`io`, `debug`, `luajit`, ...): what the report counts by.
    class: String,
}

/// One suite's entries of `skips.txt`, and which of them the run met.
struct Skips {
    entries: Vec<Entry>,
    used: RefCell<Vec<bool>>,
}

impl Skips {
    fn of(suite: &str) -> Skips {
        let text = std::fs::read_to_string(suites().join("skips.txt")).expect("the skip list");
        let mut entries = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let entry = parse_entry(i + 1, line);
            if let Some(file) = entry.file.strip_prefix(&format!("{suite}/")) {
                entries.push(Entry {
                    file: file.to_owned(),
                    ..entry
                });
            }
        }
        let used = RefCell::new(vec![false; entries.len()]);
        Skips { entries, used }
    }

    /// The entry for this part of this file, marked as met.
    fn find(&self, file: &str, part: &Part) -> Option<&Entry> {
        let i = self
            .entries
            .iter()
            .position(|e| e.file == file && e.part == *part)?;
        self.used.borrow_mut()[i] = true;
        Some(&self.entries[i])
    }

    /// The line ranges of this file, marked as met.
    fn ranges(&self, file: &str) -> Vec<&Entry> {
        let mut ranges = Vec::new();
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.file == file && matches!(entry.part, Part::Lines(..)) {
                self.used.borrow_mut()[i] = true;
                ranges.push(entry);
            }
        }
        ranges
    }

    fn unused(&self) -> Vec<String> {
        let used = self.used.borrow();
        self.entries
            .iter()
            .zip(used.iter())
            .filter(|(_, used)| !**used)
            .map(|(entry, _)| format!("skips.txt:{}: matches nothing", entry.line))
            .collect()
    }
}

fn parse_entry(line: usize, text: &str) -> Entry {
    let bad = |what: &str| -> ! { panic!("skips.txt:{line}: {what}: {text}") };
    let (head, reason) = text
        .split_once(" -- ")
        .unwrap_or_else(|| bad("no ` -- REASON`"));
    let (kind, target) = head
        .trim()
        .split_once(' ')
        .unwrap_or_else(|| bad("no target"));
    let kind = match kind {
        "skip" => Kind::Skip,
        "omit" => Kind::Omit,
        _ => bad("neither skip nor omit"),
    };
    let target = target.trim();
    let (file, part) = if let Some((file, case)) = target.split_once('#') {
        (file, Part::Case(case.to_owned()))
    } else if let Some((file, range)) = target.rsplit_once(':') {
        let (from, to) = range
            .split_once('-')
            .unwrap_or_else(|| bad("a range is FROM-TO"));
        let number = |n: &str| {
            n.parse()
                .unwrap_or_else(|_| bad("a range is two line numbers"))
        };
        let (from, to) = (number(from), number(to));
        if from == 0 || from > to {
            bad("a range runs from line 1 or later to a line no earlier");
        }
        (file, Part::Lines(from, to))
    } else {
        (target, Part::File)
    };
    let class = reason
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if class.is_empty() || class.contains(' ') {
        bad("a reason starts with its class: `CLASS: why`");
    }
    Entry {
        line,
        kind,
        file: file.to_owned(),
        part,
        class,
    }
}

/// What one suite's run came to: passes, what the skip list kept out, and everything that went otherwise.
#[derive(Default)]
struct Tally {
    passed: usize,
    skipped: BTreeMap<String, usize>,
    omitted: BTreeMap<String, usize>,
    problems: Vec<String>,
}

impl Tally {
    fn report(self, suite: &str, unit: &str, skips: &Skips) {
        let classes = |counts: &BTreeMap<String, usize>| {
            let total: usize = counts.values().sum();
            let each: Vec<String> = counts.iter().map(|(c, n)| format!("{c} {n}")).collect();
            format!("{total} ({})", each.join(", "))
        };
        println!(
            "{suite}: {} {unit} passed, {} skipped, {} omitted",
            self.passed,
            classes(&self.skipped),
            classes(&self.omitted)
        );
        let mut problems = self.problems;
        problems.extend(skips.unused());
        assert!(
            problems.is_empty(),
            "{suite}: {} problems:\n{}",
            problems.len(),
            problems.join("\n")
        );
    }

    fn skip(&mut self, entry: &Entry) {
        let counts = match entry.kind {
            Kind::Skip => &mut self.skipped,
            Kind::Omit => &mut self.omitted,
        };
        *counts.entry(entry.class.clone()).or_default() += 1;
    }
}

/// How each call of an interpreter ended, and how long the interpreter ran.
struct Ran {
    ended: Vec<Result<(), Error>>,
    elapsed: Duration,
}

/// Runs a job on a fresh interpreter of its own thread, with `require` reading below the suite's folder. What the
/// suites print goes nowhere: they pass or fail by their errors. Under wasm, which has one thread, on the calling
/// thread (`tools/wasm.sh` gives the run a hang guard of its own).
fn isolated<F>(suite: &str, name: &str, job: F) -> Ran
where
    F: FnOnce(&mut Lua) -> Vec<Result<(), Error>> + Send + 'static,
{
    let folder = suites().join(suite);
    let interpret = move || {
        let keys = Keys {
            hash: [5, 5],
            random: 5,
        };
        let began = Instant::now();
        let mut lua = Lua::new(LIMITS, keys).expect("an interpreter");
        let loader = FolderLoader::new(&folder, LIMITS.module_size as u64);
        lua.set_loader(loader.expect("the suite's folder"));
        let ended = job(&mut lua);
        let elapsed = began.elapsed();
        Ran { ended, elapsed }
    };
    if cfg!(target_os = "wasi") {
        return interpret();
    }
    let (done, outcome) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(STACK)
        .spawn(move || {
            done.send(interpret())
                .expect("the test waits for the result")
        })
        .expect("a suite thread");
    outcome
        .recv_timeout(DEADLINE)
        .unwrap_or_else(|_| panic!("{suite}/{name} did not end within the hang guard"))
}

/// Applies `run` to every job, a few at once (one after another under wasm).
fn parallel<J: Sync, T: Send>(jobs: &[J], run: impl Fn(&J) -> T + Sync) -> Vec<T> {
    if cfg!(target_os = "wasi") {
        return jobs.iter().map(run).collect();
    }
    let next = AtomicUsize::new(0);
    let results = Mutex::new((0..jobs.len()).map(|_| None).collect::<Vec<Option<T>>>());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let result = run(job);
                    results.lock().expect("no worker panicked")[i] = Some(result);
                }
            });
        }
    });
    let results = results.into_inner().expect("no worker panicked");
    results
        .into_iter()
        .map(|r| r.expect("every job ran"))
        .collect()
}

/// The text a failure report shows: the error's first line and the frames it went through.
fn describe(error: &Error) -> String {
    let text = format!("{error}");
    let mut lines = text.lines();
    let head = lines.next().unwrap_or_default().to_owned();
    let frames: Vec<&str> = lines.map(|l| l.trim().trim_start_matches("in ")).collect();
    if frames.is_empty() {
        return head;
    }
    format!("{head} (at {})", frames.join(" < "))
}

/// The lines of `chunk` an error names: its position, and the frames of its traceback.
fn lines_named(error: &Error, chunk: &str) -> Vec<u32> {
    let at = |text: &str| -> Option<u32> {
        let rest = text.strip_prefix(chunk)?.strip_prefix(':')?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    match error {
        Error::Syntax { chunk: c, line, .. } if c == chunk => vec![*line],
        // A message may carry several positions in front (`coroutine.wrap` adds its caller's): any of them counts.
        Error::Runtime { message, traceback } => message
            .match_indices(chunk)
            .map(|(i, _)| &message[i..])
            .chain(traceback.iter().map(String::as_str))
            .filter_map(at)
            .collect(),
        _ => Vec::new(),
    }
}

/// The source with these line ranges blanked, every other line keeping its number.
fn blanked(source: &[u8], ranges: &[(u32, u32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(source.len());
    for (i, line) in source.split_inclusive(|&c| c == b'\n').enumerate() {
        let number = u32::try_from(i + 1).expect("a file of fewer than 4 billion lines");
        if ranges
            .iter()
            .any(|&(from, to)| (from..=to).contains(&number))
        {
            if line.ends_with(b"\n") {
                out.push(b'\n');
            }
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

/// A whole-file suite: every file runs on its own interpreter under the suite's driver.
struct FileSuite {
    name: &'static str,
    extension: &'static str,
    /// The driver chunk that runs a file: it gets the compiled file as `...` and fails unless it ran as expected.
    driver: fn(&str) -> &'static str,
    /// What runs before every file: the names the suite's own interpreter has that luagee's dialect lacks.
    prelude: &'static str,
}

/// One run of a file: as listed (`lifted` none), or with one skipped range put back, which must fail inside it.
struct FileRun {
    file: String,
    source: Vec<u8>,
    lifted: Option<(usize, u32, u32)>,
    whole_skip: Option<usize>,
}

impl FileSuite {
    fn run(&self) {
        let skips = Skips::of(self.name);
        let mut tally = Tally::default();
        let runs = self.plan(&skips, &mut tally);
        let outcomes = parallel(&runs, |run| self.run_file(run));
        for (run, (ended, elapsed)) in runs.iter().zip(outcomes) {
            let shown = match (&ended, run.lifted, run.whole_skip) {
                (Ok(()), None, None) => "passed".to_owned(),
                (_, Some((_, from, to)), _) => format!("{from}-{to} put back: fails as listed"),
                (_, None, Some(_)) => "skipped: fails as listed".to_owned(),
                (Err(_), None, None) => "FAILED".to_owned(),
            };
            println!(
                "  {}/{}: {shown} ({:.1} s)",
                self.name,
                run.file,
                elapsed.as_secs_f32()
            );
            self.judge(run, ended, &mut tally);
        }
        tally.report(self.name, "files", &skips);
    }

    fn plan(&self, skips: &Skips, tally: &mut Tally) -> Vec<FileRun> {
        let folder = suites().join(self.name);
        let mut files: Vec<String> = std::fs::read_dir(&folder)
            .expect("the suite's folder")
            .map(|entry| entry.expect("a suite entry").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| name.ends_with(self.extension))
            .collect();
        files.sort();
        let mut runs = Vec::new();
        for file in files {
            let source = std::fs::read(folder.join(&file)).expect("a suite file");
            let ranges = skips.ranges(&file);
            if let Some(entry) = skips.find(&file, &Part::File) {
                tally.skip(entry);
                assert!(
                    ranges.is_empty(),
                    "skips.txt:{}: a whole file has no ranges",
                    entry.line
                );
                if entry.kind == Kind::Skip {
                    runs.push(FileRun {
                        file,
                        source,
                        lifted: None,
                        whole_skip: Some(entry.line),
                    });
                }
                continue;
            }
            let all: Vec<(u32, u32)> = ranges.iter().map(|e| range_of(e)).collect();
            for (i, entry) in ranges.iter().enumerate() {
                tally.skip(entry);
                if entry.kind == Kind::Skip {
                    let others: Vec<(u32, u32)> =
                        (0..all.len()).filter(|&j| j != i).map(|j| all[j]).collect();
                    let (from, to) = all[i];
                    runs.push(FileRun {
                        file: file.clone(),
                        source: blanked(&source, &others),
                        lifted: Some((entry.line, from, to)),
                        whole_skip: None,
                    });
                }
            }
            runs.push(FileRun {
                source: blanked(&source, &all),
                file,
                lifted: None,
                whole_skip: None,
            });
        }
        runs
    }

    fn run_file(&self, run: &FileRun) -> (Result<(), Error>, Duration) {
        let driver = (self.driver)(&run.file);
        let (file, source, prelude) = (run.file.clone(), run.source.clone(), self.prelude);
        let ran = isolated(self.name, &run.file, move |lua| {
            let ended = (|| {
                lua.exec("prelude", prelude.as_bytes())?;
                let main = lua.load(&file, &source)?;
                lua.pin(main).expect("root fits");
                let driver = lua.load("driver", driver.as_bytes())?;
                lua.call(driver, &[main]).map(|_| ())
            })();
            vec![ended]
        });
        let ended = ran.ended.into_iter().next().expect("the run's one call");
        (ended, ran.elapsed)
    }

    fn judge(&self, run: &FileRun, ended: Result<(), Error>, tally: &mut Tally) {
        let file = &run.file;
        match (run.whole_skip, run.lifted, ended) {
            (None, None, Ok(())) => tally.passed += 1,
            (None, None, Err(error)) => {
                tally
                    .problems
                    .push(format!("{}/{file}: {}", self.name, describe(&error)));
            }
            (Some(line), _, Ok(())) => {
                tally
                    .problems
                    .push(format!("skips.txt:{line}: {}/{file} passes now", self.name));
            }
            (Some(_), _, Err(_)) => {}
            (None, Some((line, ..)), Ok(())) => tally.problems.push(format!(
                "skips.txt:{line}: {}/{file} passes with the range put back",
                self.name
            )),
            (None, Some((line, from, to)), Err(error)) => {
                let named = lines_named(&error, file);
                let inside = named.iter().any(|n| (from..=to).contains(n));
                if !inside && !matches!(error, Error::Limit(_)) {
                    tally.problems.push(format!(
                        "skips.txt:{line}: {}/{file} with {from}-{to} put back fails elsewhere: {}",
                        self.name,
                        describe(&error)
                    ));
                }
            }
        }
    }
}

fn range_of(entry: &Entry) -> (u32, u32) {
    match entry.part {
        Part::Lines(from, to) => (from, to),
        _ => unreachable!("only ranges are listed as ranges"),
    }
}

/// The PUC-Rio driver (`all.lua`): the files run as it runs them, with the results it checks.
fn puc_driver(file: &str) -> &'static str {
    match file {
        "calls.lua" => "local main = ... assert(main() == deep and deep)",
        "attrib.lua" => "local main = ... assert(main() == 27)",
        "locals.lua" => "local main = ... assert(main() == 5)",
        "events.lua" => "local main = ... assert(main() == 12)",
        "verybig.lua" => "local main = ... assert(main() == 10)",
        "big.lua" => {
            "local main = ... local f = coroutine.wrap(main) assert(f() == 'b') assert(f() == 'a')"
        }
        _ => "local main = ... main()",
    }
}

/// Luau's conformance driver: a file passes when it returns "OK".
fn luau_driver(_: &str) -> &'static str {
    "local main = ... local result = main() if result ~= 'OK' then error('returned ' .. tostring(result), 0) end"
}

#[test]
fn the_puc_rio_lua_5_1_tests_pass() {
    puc_rio();
}

/// The PUC-Rio Lua 5.1 tests (also what `examples/corpus.rs suites` runs under wasm).
pub fn puc_rio() {
    FileSuite {
        name: "lua-5.1-tests",
        extension: ".lua",
        driver: puc_driver,
        // PUC Lua 5.1 keeps 5.0's `math.mod` (LUA_COMPAT_MOD); LuaJIT, whose dialect luagee follows, has `fmod` only.
        // The suite times its sorts with os.clock, which luagee has not: it reads 0.
        prelude: "math.mod = math.fmod os = {clock = function() return 0 end}",
    }
    .run();
}

#[test]
fn the_luau_conformance_scripts_in_lua_5_1_pass() {
    luau();
}

/// The Luau conformance scripts that are Lua 5.1.
pub fn luau() {
    FileSuite {
        name: "luau-conformance",
        extension: ".luau",
        driver: luau_driver,
        // Luau keeps 5.1's `unpack` under Lua 5.2's name, and has 5.2's `table.pack`.
        prelude: "table.unpack = unpack table.pack = function(...) return {n = select('#', ...), ...} end",
    }
    .run();
}

const LUAJIT: &str = "luajit-test-cleanup";

/// What LuaJIT's `test.lua` finds out about the interpreter, as luagee answers: Lua 5.1 with LuaJIT 2.1's `bit`,
/// `table.new`, `goto` and hexadecimal floats. `jit` and the optimisation flags stand for the trees a JIT build runs: they check
/// results only, which an interpreter gives as well (LuaJIT with `-joff` passes them).
fn tag(name: &str) -> Option<f64> {
    match name {
        "lua" => Some(5.1),
        "luajit" => Some(2.1),
        "bit" | "table.new" | "goto" | "hexfloat" => Some(1.0),
        "jit" | "dse" | "fold" | "fwd" | "fuse" | "loop" | "sink" => Some(1.0),
        _ => None,
    }
}

/// Whether metadata (`+tag`, `-tag`, `+tag>=2.1`) admits a test, as `test.lua`'s `want_meta` decides.
fn wanted(meta: &str) -> bool {
    let bytes = meta.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let polarity = bytes[i];
        let name_len = bytes[i + 1..]
            .iter()
            .take_while(|c| !b" <>=".contains(c))
            .count();
        if !b"+-".contains(&polarity) || name_len == 0 {
            i += 1;
            continue;
        }
        let name = &meta[i + 1..i + 1 + name_len];
        let cond_len = bytes[i + 1 + name_len..]
            .iter()
            .take_while(|c| b"<>=0123456789.".contains(c))
            .count();
        let cond = &meta[i + 1 + name_len..i + 1 + name_len + cond_len];
        i += 1 + name_len + cond_len;
        let holds = match (tag(name), cond) {
            (None, _) => false,
            (Some(_), "") => true,
            (Some(value), cond) => compare(value, cond),
        };
        if holds != (polarity == b'+') {
            return false;
        }
    }
    true
}

fn compare(value: f64, cond: &str) -> bool {
    let op_len = cond.bytes().take_while(|c| b"<>=".contains(c)).count();
    let (op, number) = cond.split_at(op_len);
    let number: f64 = number
        .parse()
        .unwrap_or_else(|_| panic!("a version condition: {cond}"));
    match op {
        "<" => value < number,
        "<=" => value <= number,
        ">" => value > number,
        ">=" => value >= number,
        "==" => value == number,
        _ => panic!("a version condition: {cond}"),
    }
}

/// Where the metadata of an index line or a case's name starts: the first run of spaces before one of `sigils`.
fn meta_start(info: &str, sigils: &[u8]) -> Option<usize> {
    let bytes = info.as_bytes();
    (0..bytes.len()).find(|&i| {
        let spaces = bytes[i..].iter().take_while(|&&c| c == b' ').count();
        spaces > 0 && bytes.get(i + spaces).is_some_and(|c| sigils.contains(c))
    })
}

/// A file's cases as `test.lua`'s `scan_tests` makes them: the chunk returns `{info, function, ...}`, each
/// `do --- info` at the start of a line opening a case's function. A case whose metadata leaves it out and that
/// says `!lex` becomes a comment, as its syntax may be another dialect's.
fn split_cases(source: &str) -> (String, Vec<String>) {
    const MARKER: &str = "do";
    let mut code = String::with_capacity(source.len() + 1024);
    let mut infos = Vec::new();
    let mut prefix = String::from("return {");
    let mut rest = source;
    while let Some(at) = rest.find(MARKER) {
        let (before, from) = rest.split_at(at);
        code.push_str(before);
        let Some((matched, info)) = marker(from) else {
            code.push_str(MARKER);
            rest = &from[MARKER.len()..];
            continue;
        };
        let line_start = code.is_empty() || code.ends_with('\n');
        rest = &from[matched.len() + info.len()..];
        if !line_start {
            code.push_str(matched);
            code.push_str(info);
            continue;
        }
        let quoted = info
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\0', "\\000");
        code.push_str(&format!("{prefix}\"{quoted}\",function()"));
        prefix = String::from(",");
        if info.contains(" !lex") && !meta_start(info, b"-+@!").is_some_and(|m| wanted(&info[m..]))
        {
            code.push_str("end--[========[");
            prefix = format!("]========]{prefix}");
        }
        infos.push(info.to_owned());
    }
    code.push_str(rest);
    assert!(prefix.ends_with(','), "no tests found");
    code.push_str(&prefix);
    code.push('}');
    (code, infos)
}

/// `do +--- +` and the rest of the line (at least a byte), if `text` starts with them.
fn marker(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut i = 2;
    let spaces = |i: usize| bytes[i..].iter().take_while(|&&c| c == b' ').count();
    let first = spaces(i);
    if first == 0 || !text[i + first..].starts_with("---") {
        return None;
    }
    i += first + 3;
    let second = spaces(i);
    if second == 0 {
        return None;
    }
    i += second;
    let info_len = bytes[i..]
        .iter()
        .take_while(|&&c| c != b'\r' && c != b'\n')
        .count();
    (info_len > 0).then(|| (&text[..i], &text[i..i + info_len]))
}

/// A LuaJIT case to run: its place among the file's cases, and whether it gets a private `_G`.
struct Case {
    index: usize,
    name: String,
    private: bool,
    /// The skip list's line when the case is listed as a skip: then it must fail.
    skip: Option<usize>,
}

struct LuajitFile {
    file: String,
    code: String,
    cases: Vec<Case>,
}

/// `test.lua`'s `seal_globals`: a case that assigns a global fails.
const SEAL: &str = r#"
local sealed_mt = {__newindex = function()
  error("Tests should not mutate global state", 3)
end}
local function seal(t)
  if getmetatable(t) then return end
  setmetatable(t, sealed_mt)
  for k, v in pairs(t) do
    if type(v) == "table" then seal(v) end
  end
end
seal(_G)
if getmetatable(package.loaded) == sealed_mt then
  setmetatable(package.loaded, nil)
end
"#;

/// Runs a case of the file's table, with a private `_G` (`!private_G`) as `test.lua` gives it.
const CASE: &str = r#"
local tests = ...
return function(i, private)
  local test = tests[2 * i]
  if private then
    local G = setmetatable({}, {__index = _G})
    G._G = G
    setfenv(test, G)
  end
  test()
end
"#;

fn luajit_plan(dir: &str, skips: &Skips, tally: &mut Tally, files: &mut Vec<LuajitFile>) {
    let folder = suites().join(LUAJIT).join(dir);
    let index = std::fs::read_to_string(folder.join("index")).expect("an index");
    for line in index.lines().filter(|l| !l.is_empty()) {
        let (name, meta) = match meta_start(line, b"-+@") {
            Some(m) => (&line[..m], &line[m..]),
            None => (line, ""),
        };
        let path = format!("{dir}{name}");
        let entry = skips.find(&path, &Part::File);
        if let Some(entry) = entry {
            tally.skip(entry);
            if entry.kind == Kind::Skip {
                tally.problems.push(format!(
                    "skips.txt:{}: a LuaJIT file is omitted or skipped by case",
                    entry.line
                ));
            }
        }
        match (wanted(meta), entry) {
            (_, Some(_)) => {}
            (false, None) => tally.problems.push(format!(
                "{LUAJIT}/{path}: its metadata `{meta}` leaves it out; omit it"
            )),
            (true, None) if name.ends_with(".lua") => files.push(luajit_file(&path, skips, tally)),
            (true, None) => luajit_plan(&format!("{path}/"), skips, tally, files),
        }
    }
}

fn luajit_file(path: &str, skips: &Skips, tally: &mut Tally) -> LuajitFile {
    let source =
        std::fs::read_to_string(suites().join(LUAJIT).join(path)).expect("a LuaJIT test file");
    let (code, infos) = split_cases(&source);
    let mut cases = Vec::new();
    for (i, info) in infos.iter().enumerate() {
        let (name, meta) = match meta_start(info, b"-+@!") {
            Some(m) => (&info[..m], &info[m..]),
            None => (info.as_str(), ""),
        };
        let entry = skips.find(path, &Part::Case(name.to_owned()));
        if let Some(entry) = entry {
            tally.skip(entry);
        }
        match (wanted(meta), entry) {
            (_, Some(entry)) if entry.kind == Kind::Omit => {}
            (false, _) => tally.problems.push(format!(
                "{LUAJIT}/{path}#{name}: its metadata `{meta}` leaves it out; omit it"
            )),
            (true, entry) => cases.push(Case {
                index: i + 1,
                name: name.to_owned(),
                private: meta.contains("!private_G"),
                skip: entry.map(|e| e.line),
            }),
        }
    }
    LuajitFile {
        file: path.to_owned(),
        code,
        cases,
    }
}

fn run_luajit_file(file: &LuajitFile) -> Ran {
    let (name, code) = (file.file.clone(), file.code.clone());
    let cases: Vec<(usize, bool)> = file.cases.iter().map(|c| (c.index, c.private)).collect();
    isolated(LUAJIT, &file.file, move |lua| {
        let runner = (|| {
            let case = lua.load("case", CASE.as_bytes())?;
            lua.pin(case).expect("root fits");
            let chunk = lua.load(&name, code.as_bytes())?;
            lua.pin(chunk).expect("root fits");
            lua.exec("seal", SEAL.as_bytes())?;
            let tests = lua.call(chunk, &[])?;
            let runner = lua.call(case, &tests)?;
            let runner = runner.first().copied().unwrap_or_default();
            lua.pin(runner).expect("root fits");
            Ok(runner)
        })();
        cases
            .iter()
            .map(|&(index, private)| {
                let runner = runner.clone()?;
                lua.begin_tick();
                let index = Value::Number(f64::from(u32::try_from(index).expect("a few cases")));
                lua.call(runner, &[index, Value::Boolean(private)])
                    .map(|_| ())
            })
            .collect()
    })
}

#[test]
fn the_luajit_test_cleanup_cases_pass() {
    luajit();
}

/// LuaJIT's test-cleanup cases.
pub fn luajit() {
    let skips = Skips::of(LUAJIT);
    let mut tally = Tally::default();
    let mut files = Vec::new();
    luajit_plan("", &skips, &mut tally, &mut files);
    let outcomes = parallel(&files, run_luajit_file);
    for (file, ran) in files.iter().zip(outcomes) {
        let failed = ran.ended.iter().filter(|e| e.is_err()).count();
        let elapsed = ran.elapsed.as_secs_f32();
        println!(
            "  {LUAJIT}/{}: {} cases, {failed} failing ({elapsed:.1} s)",
            file.file,
            file.cases.len()
        );
        for (case, ended) in file.cases.iter().zip(ran.ended) {
            let target = format!("{LUAJIT}/{}#{}", file.file, case.name);
            match (case.skip, ended) {
                (None, Ok(())) => tally.passed += 1,
                (None, Err(error)) => tally
                    .problems
                    .push(format!("{target}: {}", describe(&error))),
                (Some(line), Ok(())) => tally
                    .problems
                    .push(format!("skips.txt:{line}: {target} passes now")),
                (Some(_), Err(_)) => {}
            }
        }
    }
    tally.report(LUAJIT, "cases", &skips);
}
