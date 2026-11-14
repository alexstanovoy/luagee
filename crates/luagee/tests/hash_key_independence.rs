//! What a script can observe, and what it costs, never depends on the hash key; the random stream is pinned.
//!
//! The hash key only seeds the keyed hash that spreads table keys over the index. These tests run the same programs
//! under several keys and require identical output, outcome, fuel spent and memory counted, also when the program is
//! cut short by a fuel or a memory limit.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use luagee::{FolderLoader, Keys, Limits, Lua};

const HASH_KEYS: [[u64; 2]; 6] = [
    [0, 0],
    [1, 2],
    [u64::MAX, u64::MAX],
    [0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210],
    [0xdead_beef, 42],
    [7, 7],
];
const RANDOM_SEED: u64 = 7;
const STACK: usize = 16 << 20;
const WORKERS: usize = 8;

/// Everything of a run that a script or a host can see.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    ended: String,
    printed: Vec<u8>,
    fuel_spent: u64,
    memory: usize,
    memory_after_collection: usize,
}

fn run(source: &[u8], limits: Limits, hash: [u64; 2], modules: Option<&Path>) -> Observed {
    let printed = Rc::new(RefCell::new(Vec::new()));
    let mut lua = Lua::new(
        limits,
        Keys {
            hash,
            random: RANDOM_SEED,
        },
    )
    .expect("an interpreter");
    let sink = printed.clone();
    lua.set_print(Box::new(move |line| {
        sink.borrow_mut().extend_from_slice(line)
    }));
    if let Some(folder) = modules {
        lua.set_loader(FolderLoader::new(folder, limits.module_size as u64).expect("a folder"));
    }
    let ended = format!("{:?}", lua.exec("program", source).map(|_| ()));
    let fuel_spent = limits.fuel_per_tick - lua.tick_fuel_left();
    let memory = lua.memory();
    lua.begin_tick();
    let collected = format!("{:?}", lua.collect());
    let memory_after_collection = lua.memory();
    Observed {
        ended: format!("{ended} collect={collected}"),
        printed: printed.take(),
        fuel_spent,
        memory,
        memory_after_collection,
    }
}

fn limited(limits: Limits, fuel: u64) -> Limits {
    limits.with_fuel_per_call(fuel).with_fuel_per_tick(fuel)
}

/// Runs `source` under every key, at its own limits, at three fuel cuts and at a memory cut, and returns what
/// differs from the first key (empty when nothing does).
fn differences(
    name: &str,
    source: &[u8],
    limits: Limits,
    keys: &[[u64; 2]],
    modules: Option<&Path>,
) -> Vec<String> {
    let reference = run(source, limits, keys[0], modules);
    let spent = reference.fuel_spent.max(2);
    let mut cases = vec![("unlimited".to_owned(), limits)];
    for cut in [spent - 1, spent / 2, spent / 3 + 1] {
        cases.push((format!("fuel {cut}"), limited(limits, cut)));
    }
    let memory_cut = (reference.memory / 2).max(Limits::MIN_MEMORY);
    cases.push((
        format!("memory {memory_cut}"),
        limits.with_memory(memory_cut),
    ));
    let mut found = Vec::new();
    for (case, limits) in cases {
        let expected = run(source, limits, keys[0], modules);
        for &hash in &keys[1..] {
            let got = run(source, limits, hash, modules);
            if got != expected {
                found.push(format!(
                    "{name} at {case}: key {hash:?} differs from {:?}:\n  {got:?}\n  {expected:?}",
                    keys[0]
                ));
            }
        }
    }
    found
}

fn parallel<T: Sync, R: Send>(jobs: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new((0..jobs.len()).map(|_| None).collect::<Vec<Option<R>>>());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            std::thread::Builder::new()
                .stack_size(STACK)
                .spawn_scoped(scope, || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(i) else { break };
                        let result = work(job);
                        results.lock().expect("no worker panicked")[i] = Some(result);
                    }
                })
                .expect("a worker");
        }
    });
    let results = results.into_inner().expect("no worker panicked");
    results
        .into_iter()
        .map(|r| r.expect("every job ran"))
        .collect()
}

const MEMORY_CAPPED: &str = "runs into the memory cap";

const PROGRAMS: &[(&str, &str)] = &[
    (
        "string keys: insert, delete, reinsert, clear during traversal",
        r##"
        local t = {}
        for i = 1, 3000 do t["key" .. i] = i end
        for i = 1, 3000, 3 do t["key" .. i] = nil end
        for i = 1, 500 do t["new" .. i] = -i end
        for i = 1, 3000, 6 do t["key" .. i] = i end
        local n, sum, order = 0, 0, {}
        for k, v in pairs(t) do
            n = n + 1
            sum = sum + v
            if n % 97 == 0 then order[#order + 1] = k end
        end
        print(n, sum, table.concat(order, ","))
        local c = 0
        for k in pairs(t) do t[k] = nil c = c + 1 end
        print(c, next(t), collectgarbage("count"))
        "##,
    ),
    (
        "number keys: array and hash migration, floats, negatives, huge keys",
        r##"
        local t = {}
        for i = 2000, 1, -1 do t[i] = i end
        print(#t)
        local u = {}
        for i = 1, 1000 do u[i * 0.5] = i end
        for i = 1, 300 do u[-i] = i end
        u[0] = 0 u[1e6] = 1 u[2^53] = 2 u[1 / 0] = 3 u[-1 / 0] = 4
        local v = {}
        for i = 1, 100 do v[i] = i end
        for i = 1, 100, 2 do v[i] = nil end
        v[101] = 1 v[200] = 1
        for _, x in ipairs({t, u, v}) do
            local keys = {}
            for k in pairs(x) do keys[#keys + 1] = tostring(k) end
            print(#keys, #x, table.maxn(x), table.concat(keys, " ", 1, math.min(#keys, 400)))
        end
        "##,
    ),
    (
        "object keys: tables, functions, threads, booleans, identities",
        r##"
        local keys, m, shown = {}, {}, {}
        for i = 1, 300 do
            keys[i] = (i % 3 == 0) and {} or (i % 3 == 1) and function() return i end or coroutine.create(print)
            m[keys[i]] = i
        end
        m[true] = 1 m[false] = 2
        for k, v in pairs(m) do shown[#shown + 1] = tostring(k) .. "=" .. v end
        print(table.concat(shown, " "))
        print(string.format("%p %p %p", m, "abc", keys[1]), tostring(print), tostring({}))
        "##,
    ),
    (
        "strings and errors: interning, sorting, collection, messages",
        r##"
        local s = {}
        for i = 1, 5000 do s[#s + 1] = ("x"):rep(i % 17) .. i end
        table.sort(s)
        print(s[1], s[#s], #table.concat(s))
        local seen = {}
        for _, v in ipairs(s) do seen[v] = true end
        collectgarbage()
        print(collectgarbage("count"), gcinfo())
        print(pcall(function() local t = {} return t.a.b end))
        print(pcall(next, {}, "missing"))
        print(pcall(function() local t = setmetatable({}, {__index = seen}) return t.x1() end))
        print(pcall(error, setmetatable({}, {__tostring = function() return "boxed" end})))
        print(select("#", unpack(s, 1, 200)))
        "##,
    ),
    (
        "random draws feed table keys",
        r##"
        local t = {}
        for i = 1, 2000 do t[math.random(1, 100000)] = math.random() end
        local order = {}
        for k in pairs(t) do order[#order + 1] = k end
        print(#order, table.concat(order, ",", 1, 100))
        math.randomseed(99)
        print(math.random(1000), math.random(1000))
        "##,
    ),
    (
        "runs into the memory cap through string keys",
        r##"
        local t, i = {}, 0
        while true do
            i = i + 1
            t["k" .. i] = {i}
            if i % 50 == 0 then print(i) end
        end
        "##,
    ),
    (
        "coroutines and metatables over keyed tables",
        r##"
        local weak = setmetatable({}, {__index = function(t, k) rawset(t, k, #k) return #k end})
        local co = coroutine.wrap(function()
            for i = 1, 1500 do coroutine.yield(weak["item" .. i % 700]) end
        end)
        local total = 0
        for _ = 1, 1500 do total = total + co() end
        local n = 0
        for _ in pairs(weak) do n = n + 1 end
        print(total, n)
        "##,
    ),
];

#[test]
fn programs_observe_and_cost_the_same_under_every_hash_key() {
    let limits = Limits::DEFAULT
        .with_fuel_per_call(50_000_000)
        .with_fuel_per_tick(50_000_000);
    // The memory-capped program has its own test: at the default memory it would run for a long time.
    let programs: Vec<_> = PROGRAMS
        .iter()
        .filter(|(name, _)| !name.starts_with(MEMORY_CAPPED))
        .collect();
    let found = parallel(&programs, |&&(name, source)| {
        let reference = run(source.as_bytes(), limits, HASH_KEYS[0], None);
        assert!(
            reference.ended.starts_with("Ok(())"),
            "{name} must run to its end: {}",
            reference.ended
        );
        assert!(!reference.printed.is_empty(), "{name} prints");
        eprintln!(
            "{name}: fuel {} memory {} printed {} bytes",
            reference.fuel_spent,
            reference.memory,
            reference.printed.len()
        );
        differences(name, source.as_bytes(), limits, &HASH_KEYS, None)
    });
    let found: Vec<String> = found.into_iter().flatten().collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}

fn corpus_scripts(folder: &str) -> Vec<(PathBuf, PathBuf)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(folder);
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&folder)
        .expect("a corpus folder")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "lua"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "{} holds scripts", folder.display());
    scripts.into_iter().map(|s| (s, folder.clone())).collect()
}

fn corpus_differences(folder: &str, limits: Limits, keys: &[[u64; 2]]) -> Vec<String> {
    let scripts = corpus_scripts(folder);
    let found = parallel(&scripts, |(script, folder)| {
        let source = std::fs::read(script).expect("a script");
        let name = script.file_name().expect("a name").to_string_lossy();
        differences(&name, &source, limits, keys, Some(folder))
    });
    found.into_iter().flatten().collect()
}

#[test]
fn conformance_corpus_is_identical_under_every_hash_key() {
    let limits = Limits::DEFAULT
        .with_fuel_per_call(1 << 36)
        .with_fuel_per_tick(1 << 36);
    let found = corpus_differences("conformance", limits, &HASH_KEYS[..4]);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn hostile_corpus_ends_identically_under_every_hash_key() {
    let limits = Limits::DEFAULT
        .with_fuel_per_call(5_000_000)
        .with_fuel_per_tick(5_000_000)
        .with_memory(16 << 20);
    let found = corpus_differences("hostile", limits, &HASH_KEYS[..3]);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn memory_capped_program_stops_at_the_same_step_under_every_key() {
    let source = PROGRAMS
        .iter()
        .find(|(name, _)| name.starts_with(MEMORY_CAPPED))
        .expect("the program")
        .1;
    let limits = Limits::DEFAULT.with_memory(256 << 10);
    let reference = run(source.as_bytes(), limits, HASH_KEYS[0], None);
    assert!(
        reference.ended.contains("Memory"),
        "the program must end at the memory cap: {}",
        reference.ended
    );
    for hash in HASH_KEYS {
        assert_eq!(run(source.as_bytes(), limits, hash, None), reference);
    }
}

/// The stream is xoshiro256** seeded by SplitMix64; these values come from an independent Python implementation of
/// both, so they hold on every platform and in every release unless the stream is deliberately changed.
#[test]
fn random_stream_is_pinned_for_a_host_seed() {
    for (seed, expected) in [
        (3, "690639 640582 218263 533962 424596 399509"),
        (0, "601263 747775 103020 416590 732997 999749"),
        (
            0xdead_beef_cafe_f00d,
            "617963 85944 542395 485146 766517 856989",
        ),
    ] {
        for hash in &HASH_KEYS[..3] {
            let mut lua = Lua::new(
                Limits::DEFAULT,
                Keys {
                    hash: *hash,
                    random: seed,
                },
            )
            .expect("interpreter");
            let printed = Rc::new(RefCell::new(Vec::new()));
            let sink = printed.clone();
            lua.set_print(Box::new(move |line| {
                sink.borrow_mut().extend_from_slice(line)
            }));
            lua.exec(
                "draws",
                b"local d = {} for i = 1, 6 do d[i] = math.random(1, 1000000) end print(table.concat(d, ' '))",
            )
            .expect("runs");
            assert_eq!(
                String::from_utf8(printed.take()).expect("text"),
                format!("{expected}\n"),
                "seed {seed}"
            );
        }
    }
}
