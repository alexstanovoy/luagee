//! Objects moved while the collector runs, through every kind of holder (tables, upvalue cells, coroutine stacks,
//! metatables, environments, error values), in seeded random programs: each program checks every object it holds
//! when it runs `verify`, and its output must not depend on how the collector is paced.

use std::cell::RefCell;
use std::rc::Rc;

use luagee::{GcStress, Keys, Limits, Lua};

/// The programs' shared prelude: holders, the object maker, and `verify`, which prints a checksum of every holder
/// (and BAD for an object that is not what it was).
const PRELUDE: &str = r#"local clear = require "table.clear"
local fmt = string.format
local objs = {}              -- id -> checksum expectation (strong only for ids, not objects)
local nextid = 0
local function mk(extra)
  nextid = nextid + 1
  local o = {id = nextid, tag = "o" .. nextid, kids = {}}
  if extra then o.pad = extra end
  return o
end
local function chk(o, label)
  if type(o) ~= "table" or o.tag ~= "o" .. tostring(o.id) then print("BAD", label, type(o), o and o.id) return 0 end
  return o.id
end
local H = {}   -- hash holder, string keys
local A = {}   -- array holder
local K = {}   -- objects as keys
local M = setmetatable({}, {__index = mk()})  -- metatable holder
local CL = {}  -- closure holder
local CO = {}  -- coroutine holder
local E = {}   -- error values
local CELLS = {}
local seq = 0
local function takeOne() local k, v = next(H) if k ~= nil then H[k] = nil return v end local n = #A if n > 0 then local x = A[n] A[n] = nil return x end return mk() end
local function newcell() local c; local st = function(v) c = v end; local g = function() return c end; CELLS[#CELLS + 1] = {st, g}; return CELLS[#CELLS] end
local function sumCELLS() local s = 0 for i, c in ipairs(CELLS) do local v = c[2]() if v ~= nil then s = s + chk(v, "CELL" .. i) end end return s end
local function key() seq = seq + 1 return "k" .. seq .. "_" .. (seq * 7919 % 101) end
local function sumH() local s = 0 for k, v in pairs(H) do s = s + chk(v, "H" .. tostring(k)) end return s end
local function sumA() local s = 0 for i = 1, #A do s = s + chk(A[i], "A" .. i) end for k, v in pairs(A) do if type(k) ~= "number" then s = s + chk(v, "Ax") end end return s end
local function sumK() local s = 0 for k, v in pairs(K) do s = s + chk(k, "K") + (v == true and 0 or chk(v, "Kv")) end return s end
local function sumCL() local s = 0 for i, f in pairs(CL) do s = s + chk(f(), "CL" .. i) end return s end
local function sumE() local s = 0 for i, e in pairs(E) do s = s + chk(e, "E" .. i) end return s end
local function sumCO()
  local s = 0
  for i, c in pairs(CO) do
    if coroutine.status(c) == "suspended" then
      local ok, v = coroutine.resume(c)
      if ok and type(v) == "table" then s = s + chk(v, "CO" .. i) end
    end
  end
  return s
end
local function verify(label)
  local a, b, c, d, e = sumH(), sumA(), sumK(), sumCL(), sumE()
  print("V", label, a, b, c, d, e, sumCO(), sumCELLS(), chk(M.id and M or mk(), "M") and getmetatable(M).__index.id)
end
local function bigstack(n, o) -- recursion keeping o only on the stack
  if n == 0 then coroutine.yield(o) return end
  local a, b, c, d = o, nil, nil, nil
  bigstack(n - 1, o)
  return a
end"#;

/// What a program does between two checks: `{n}` is a count (up to 60), `{nK}` a count up to K, `{rK}` a number up
/// to K.
const ACTIONS: [&str; 50] = [
    r#"for i = 1, {n} do H[key()] = mk() end"#,
    r#"for i = 1, {n} do H[key()] = mk() A[#A + 1] = mk() end"#,
    r#"for i = 1, {n} do A[#A + 1] = mk() end"#,
    r#"for i = 1, {n} do K[mk()] = mk() end"#,
    r#"for i = 1, {n} do H[key()] = nil end for k in pairs(H) do H[k] = nil break end"#,
    r#"for k, v in pairs(H) do if seq % 3 == 0 then H[k] = nil else H[k] = mk() end seq = seq + 1 if seq % 50 == 0 then collectgarbage() end end"#,
    r#"for i = 1, {n} do local o = mk() CL[#CL + 1] = function() return o end end"#,
    r#"do local o = mk() local t = {} for i = 1, {n} do t[i] = mk() end CL[#CL + 1] = function() return t[#t] end end"#,
    r#"for i = 1, {n40} do local o = mk() local co = coroutine.create(function() local keep = o local junk = bigstack({r20}, mk()) coroutine.yield(keep) return keep end) coroutine.resume(co) CO[#CO + 1] = co end"#,
    r#"for i = 1, {n30} do local co = coroutine.wrap(function(x) local m = mk() while true do x = coroutine.yield(m) end end) co() CL[#CL + 1] = function() return co() end end"#,
    r#"for i = 1, {n20} do local co = coroutine.create(function() local a = mk() local b = coroutine.create(function() local c = mk() coroutine.yield(c) coroutine.yield(a) end) local _, x = coroutine.resume(b) coroutine.yield(x) local _, y = coroutine.resume(b) coroutine.yield(y) end) coroutine.resume(co) CO[#CO + 1] = co end"#,
    r#"for i = 1, {n20} do local co = coroutine.create(function() local keep = mk() error(keep) end) local ok, e = coroutine.resume(co) E[#E + 1] = e end"#,
    r#"for i = 1, {n20} do local ok, e = pcall(function() local o = mk() local t = {o} error(t[1]) end) E[#E + 1] = e end"#,
    r#"for i = 1, {n20} do local ok, e = xpcall(function() error(mk()) end, function(m) collectgarbage() return m end) E[#E + 1] = e end"#,
    r#"M = setmetatable({}, {__index = mk()}) collectgarbage()"#,
    r#"for i = 1, {n} do setmetatable(M, {__index = mk()}) end"#,
    r#"for i = 1, {n} do local t = setmetatable({}, {__index = function(_, k) collectgarbage() return mk() end}) local _ = t.x end"#,
    r#"clear(H)"#,
    r#"clear(A)"#,
    r#"for i = 1, {n} do table.insert(A, 1, mk()) end"#,
    r#"for i = 1, {n} do table.remove(A, 1) end"#,
    r#"for i = 1, {n} do table.remove(A) end"#,
    r#"table.sort(A, function(x, y) if seq % 5 == 0 then collectgarbage() end seq = seq + 1 return x.id < y.id end)"#,
    r#"for i = 1, {n} do A[#A + 1 + (i % 3)] = mk() end"#,
    r#"for i = 1, {n} do A[i] = A[#A] end"#,
    r#"do local s = string.rep("x", {n}) local out = s:gsub("x", function(c) if seq % 7 == 0 then collectgarbage() end seq = seq + 1 H[key()] = mk() return "y" end) end"#,
    r#"for i = 1, {n} do H[key() .. string.rep("z", {r40})] = mk() end"#,
    r#"for i = 1, {n} do local f = loadstring("local a = ... return {a, {a}, \'s{r999}\'}") if f then A[#A + 1] = {id=0,tag="o0",kids=f(mk())} end end"#,
    r#"for i = 1, {n40} do local env = {o = mk()} local f = loadstring("return o") setfenv(f, env) CL[#CL + 1] = f end"#,
    r#"collectgarbage()"#,
    r#"collectgarbage() collectgarbage()"#,
    r#"for i = 1, {n} do H[i] = mk() end for i = {n}, 1, -1 do H[i] = nil end"#,
    r#"for i = 1, {n} do local o = mk() H[key()] = o H[key()] = nil A[#A+1] = o end"#,
    r#"do local t = {} for i = 1, {n} do t[i] = mk() end for i = 1, #t do local o = t[i] t[i] = nil K[o] = true end end"#,
    r#"do local t = {} for i = 1, {n} do t[key()] = mk() end for k, v in pairs(t) do t[k] = nil H[k] = v end end"#,
    r#"do local t = {} for i = 1, {n} do t[key()] = mk() end for k, v in next, t do t[k] = mk() if seq % 11 == 0 then collectgarbage() end end for k, v in pairs(t) do A[#A+1] = v end end"#,
    r#"do local pending = {} for i = 1, {n30} do pending[i] = coroutine.create(function(a) local x = a while true do x = coroutine.yield(x) end end) end for round = 1, 3 do for i, c in ipairs(pending) do coroutine.resume(c, mk()) end collectgarbage() end for i, c in ipairs(pending) do CO[#CO + 1] = c end end"#,
    r#"for i = 1, {n30} do local u = mk() local g = function() return u end local h = function() u = mk() return g() end CL[#CL + 1] = h end"#,
    r#"for i = 1, {n20} do local ok, e = pcall(string.rep, mk(), 2) end"#,
    r#"for i = 1, {n} do local x = tostring(mk()) .. string.rep("q", i) H[x] = mk() end"#,
    r#"for i = 1, {n30} do local c = newcell() c[1](takeOne()) end"#,
    r#"for i, c in ipairs(CELLS) do c[1](takeOne()) if i % 7 == 0 then collectgarbage() end end"#,
    r#"for i, c in ipairs(CELLS) do c[1](takeOne()) end"#,
    r#"for i = 1, {n30} do setmetatable(M, {__index = takeOne()}) end"#,
    r#"for i = 1, {n30} do local f = function() return 1 end setfenv(f, {o = takeOne()}) CL[#CL + 1] = function() return getfenv(f).o end end"#,
    r#"for i = 1, {n20} do local v = takeOne() local co = coroutine.create(function() local keep = v coroutine.yield(keep) return keep end) coroutine.resume(co) CO[#CO + 1] = co end"#,
    r#"for i = 1, {n30} do K[takeOne()] = takeOne() end"#,
    r#"for i = 1, {n30} do E[#E + 1] = takeOne() end"#,
    r#"for i, co in ipairs(CO) do if coroutine.status(co) == "suspended" then local ok, v = coroutine.resume(co) if i % 3 == 0 then A[#A + 1] = v end end end"#,
    r#"for k, v in pairs(H) do local nk = key() H[k] = nil H[nk] = v if seq % 13 == 0 then collectgarbage() end seq = seq + 1 break end"#,
];

/// xorshift64*: a small, seeded, portable generator.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `low..=high`.
    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low + 1)
    }

    /// True `percent` times in a hundred.
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// The program of `seed`: 15 to 30 actions, checks between some, two whole collections and a check at the end.
fn program(seed: u64) -> String {
    let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut lines = vec![PRELUDE.to_owned()];
    for step in 0..random.range(15, 30) {
        let n = random.range(1, 60);
        let action = ACTIONS[random.range(0, ACTIONS.len() as u64 - 1) as usize]
            .replace("{n}", &n.to_string())
            .replace("{n20}", &n.min(20).to_string())
            .replace("{n30}", &n.min(30).to_string())
            .replace("{n40}", &n.min(40).to_string())
            .replace("{r20}", &random.range(1, 20).to_string())
            .replace("{r40}", &random.range(0, 40).to_string())
            .replace("{r999}", &random.range(1, 999).to_string());
        lines.push(action);
        if random.chance(45) {
            lines.push(format!("verify('s{step}')"));
        }
        if random.chance(10) {
            lines.push("collectgarbage()".to_owned());
        }
    }
    lines.push("verify('end1') collectgarbage() collectgarbage() verify('end2')".to_owned());
    lines.join("\n")
}

/// What the program prints with the collector paced as `stress` says.
fn printed(source: &str, stress: GcStress) -> Vec<String> {
    let mut lua = Lua::new(
        Limits {
            fuel_per_call: 1 << 36,
            fuel_per_tick: 1 << 36,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [5, 8],
            random: 13,
        },
    )
    .expect("state");
    let output = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&output);
    lua.set_print(Box::new(move |line| {
        sink.borrow_mut()
            .push(String::from_utf8_lossy(line).into_owned())
    }));
    lua.set_gc_stress(stress);
    if let Err(error) = lua.exec("moves", source.as_bytes()) {
        panic!("{stress:?}: {error}\n{source}");
    }
    output.take()
}

#[test]
fn objects_moved_while_collecting_are_alive_however_the_collector_is_paced() {
    for seed in 0..48 {
        let source = program(seed);
        let plain = printed(&source, GcStress::Off);
        // A hole in `A` checks as BAD with nil (the programs make holes); an object that is not what it was would
        // check as BAD with its type (a freed one would not check at all: its handle is refused).
        let corrupt = |line: &String| line.starts_with("BAD") && !line.contains("\tnil\t");
        assert!(!plain.iter().any(corrupt), "seed {seed}: {plain:?}");
        assert!(
            plain.len() >= 2,
            "seed {seed}: the program checked its objects"
        );
        assert_eq!(
            printed(&source, GcStress::Incremental),
            plain,
            "seed {seed}"
        );
    }
}
