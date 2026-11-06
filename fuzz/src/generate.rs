//! Lua programs from fuzzer bytes: every program parses, ends (loops and calls are bounded by construction), and in
//! the deterministic dialect prints the same in any correct Lua 5.1 / LuaJIT 2.1 interpreter, so two interpreters can
//! be compared on it.
//!
//! The deterministic dialect leaves out what correct interpreters may print differently: table and function
//! addresses (printed and formatted values go through `show`, which prints their type), `pairs` order, the length of
//! a table with holes (any border is a correct length: `len` prints `?` for those, and the table functions that
//! use the length leave such tables alone), and recursion deep enough to reach an interpreter's own stack bound
//! (functions call only the ones declared before them, metamethods only return their operands or constants).

use libfuzzer_sys::arbitrary::{Result, Unstructured};

/// Which programs to make.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Everything the generator knows, including what interpreters may print differently.
    Any,
    /// Only what every correct interpreter prints alike.
    Deterministic,
}

/// The variables every program declares up front.
const VARIABLES: usize = 6;
/// The functions every program declares up front; `f<i>` calls only `f<j>` for `j < i`, so nothing recurses.
const FUNCTIONS: usize = 4;
/// How deep expressions and blocks nest.
const DEPTH: u32 = 4;
/// The statements of one block.
const STATEMENTS: usize = 6;

const PRELUDE: &str = r##"local rawtostring, format = tostring, string.format
local function show(x)
  local t = type(x)
  if t == "table" or t == "function" or t == "thread" or t == "userdata" then return t end
  return rawtostring(x)
end
local function out(...)
  local parts = {}
  for i = 1, select("#", ...) do parts[i] = show((select(i, ...))) end
  print(table.concat(parts, " "))
end
"##;

/// What the deterministic dialect calls instead of what may print differently.
const DETERMINISTIC_PRELUDE: &str = r##"local tostring = show
local function fmt(f, a, b) return format(f, show(a), show(b)) end
local function len(x)
  if type(x) ~= "table" then return #x end
  local n = 0
  while rawget(x, n + 1) ~= nil do n = n + 1 end
  for k in pairs(x) do
    if type(k) == "number" and k > n then return "?" end
  end
  return #x
end
local function whole(f)
  return function(t, ...)
    if type(t) == "table" and len(t) == "?" then return "?" end
    return f(t, ...)
  end
end
local tinsert, tremove, tconcat, tunpack = whole(table.insert), whole(table.remove), whole(table.concat), whole(unpack)
"##;

/// A whole program.
pub fn program(u: &mut Unstructured, dialect: Dialect) -> Result<String> {
    let mut g = Generator {
        u,
        dialect,
        source: String::from(PRELUDE),
        function: FUNCTIONS,
        labels: 0,
    };
    if dialect == Dialect::Deterministic {
        g.source.push_str(DETERMINISTIC_PRELUDE);
    }
    g.source
        .push_str("local v0, v1, v2, v3, v4, v5 = 0, 1, \"2\", {}, {1, 2, 3}, \"x\"\n");
    for i in 0..FUNCTIONS {
        g.function = i;
        g.source.push_str(&format!("local function f{i}(a, b)\n"));
        g.block(1)?;
        g.source.push_str("  return ");
        g.expression(1)?;
        g.source.push_str("\nend\n");
    }
    g.function = FUNCTIONS;
    // The body runs protected, so that an error ends it with its message printed rather than stopping everything.
    g.source.push_str("out(pcall(function()\n");
    g.block(0)?;
    let length = if dialect == Dialect::Deterministic {
        "len"
    } else {
        "#"
    };
    g.source.push_str(&format!(
        "end))\nout(v0, v1, v2, {length}(v3), {length}(v4), v5)\n"
    ));
    Ok(g.source)
}

struct Generator<'a, 'b> {
    u: &'a mut Unstructured<'b>,
    dialect: Dialect,
    source: String,
    /// The function being generated (`FUNCTIONS` for the main body): it may call only the ones before it.
    function: usize,
    labels: u32,
}

impl Generator<'_, '_> {
    fn block(&mut self, depth: u32) -> Result<()> {
        let count = self.u.int_in_range(0..=STATEMENTS)?;
        for _ in 0..count {
            if self.u.is_empty() {
                break;
            }
            self.statement(depth)?;
        }
        Ok(())
    }

    fn statement(&mut self, depth: u32) -> Result<()> {
        let nested = depth < DEPTH;
        let choice = self.u.int_in_range(0..=13)?;
        match choice {
            0 | 1 => {
                self.variable()?;
                self.source.push_str(" = ");
                self.expression(depth + 1)?;
            }
            2 => {
                self.variable()?;
                self.source.push('[');
                self.expression(depth + 1)?;
                self.source.push_str("] = ");
                self.expression(depth + 1)?;
            }
            3 => {
                self.source.push_str("out(");
                self.arguments(depth + 1)?;
                self.source.push(')');
            }
            4 if nested => {
                self.source.push_str("if ");
                self.expression(depth + 1)?;
                self.source.push_str(" then\n");
                self.block(depth + 1)?;
                if self.u.arbitrary()? {
                    self.source.push_str("elseif ");
                    self.expression(depth + 1)?;
                    self.source.push_str(" then\n");
                    self.block(depth + 1)?;
                }
                if self.u.arbitrary()? {
                    self.source.push_str("else\n");
                    self.block(depth + 1)?;
                }
                self.source.push_str("end");
            }
            5 if nested => {
                let start = self.u.int_in_range(-3..=3)?;
                let stop = self.u.int_in_range(-3..=8)?;
                let step = *self.u.choose(&[1, 2, -1, 3])?;
                self.source
                    .push_str(&format!("for i = {start}, {stop}, {step} do\n"));
                self.block(depth + 1)?;
                self.source.push_str("end");
            }
            6 if nested => {
                let pairs = self.dialect == Dialect::Any && self.u.arbitrary()?;
                let iterator = if pairs { "pairs" } else { "ipairs" };
                self.source.push_str(&format!(
                    "for k, v in {iterator}(type(v4) == \"table\" and v4 or {{}}) do\n"
                ));
                self.block(depth + 1)?;
                self.source.push_str("end");
            }
            7 if nested => {
                // A guard bounds the loop whatever its condition does.
                self.source
                    .push_str("do local guard = 0 while guard < 4 and (");
                self.expression(depth + 1)?;
                self.source.push_str(") do guard = guard + 1\n");
                self.block(depth + 1)?;
                self.source.push_str("end end");
            }
            8 if nested => {
                self.source.push_str("out(pcall(function()\n");
                self.block(depth + 1)?;
                self.source.push_str("end))");
            }
            9 if nested => {
                self.source
                    .push_str("do local co = coroutine.wrap(function(a)\n");
                self.block(depth + 1)?;
                self.source.push_str("coroutine.yield(");
                self.expression(depth + 1)?;
                self.source.push_str(")\nreturn a end)\nout(pcall(co, ");
                self.expression(depth + 1)?;
                self.source.push_str("))\nout(pcall(co))\nend");
            }
            10 => {
                self.source.push_str("setmetatable(v3, {");
                self.metamethods()?;
                self.source.push_str("})");
            }
            11 => {
                let label = self.labels;
                self.labels += 1;
                self.source.push_str(&format!(
                    "do goto l{label}\nout(\"skipped\")\n::l{label}:: end"
                ));
            }
            12 => {
                self.source.push_str("error(");
                self.expression(depth + 1)?;
                self.source.push(')');
            }
            _ => {
                self.source.push_str("local t = ");
                self.expression(depth + 1)?;
                self.source.push_str(" out(t)");
            }
        }
        self.source.push('\n');
        Ok(())
    }

    fn metamethods(&mut self) -> Result<()> {
        const EVENTS: [&str; 12] = [
            "__index",
            "__newindex",
            "__add",
            "__sub",
            "__mul",
            "__concat",
            "__eq",
            "__lt",
            "__le",
            "__len",
            "__call",
            "__unm",
        ];
        let count = self.u.int_in_range(1..=3)?;
        for _ in 0..count {
            let event = *self.u.choose(&EVENTS)?;
            self.source.push_str(event);
            // Only operands and constants: anything more could set the metamethod off again, without bound.
            self.source.push_str(" = function(a, b) return ");
            let result = *self
                .u
                .choose(&["a", "b", "1", "\"m\"", "nil", "true", "false"])?;
            self.source.push_str(result);
            self.source.push_str(" end, ");
        }
        Ok(())
    }

    fn variable(&mut self) -> Result<()> {
        let i = self.u.int_in_range(0..=VARIABLES - 1)?;
        self.source.push_str(&format!("v{i}"));
        Ok(())
    }

    fn arguments(&mut self, depth: u32) -> Result<()> {
        let count = self.u.int_in_range(0..=3)?;
        for i in 0..count {
            if i > 0 {
                self.source.push_str(", ");
            }
            self.expression(depth)?;
        }
        Ok(())
    }

    fn expression(&mut self, depth: u32) -> Result<()> {
        if depth >= DEPTH || self.u.is_empty() {
            return self.atom();
        }
        match self.u.int_in_range(0..=9)? {
            0..=2 => self.atom(),
            3 => {
                const OPERATORS: [&str; 15] = [
                    "+", "-", "*", "/", "%", "^", "..", "==", "~=", "<", "<=", ">", ">=", "and",
                    "or",
                ];
                self.source.push('(');
                self.expression(depth + 1)?;
                let operator = *self.u.choose(&OPERATORS)?;
                self.source.push_str(&format!(" {operator} "));
                self.expression(depth + 1)?;
                self.source.push(')');
                Ok(())
            }
            4 => {
                let length = if self.dialect == Dialect::Deterministic {
                    "len"
                } else {
                    "#"
                };
                let operator = *self.u.choose(&["-", "not ", length])?;
                self.source.push_str(&format!("({operator}("));
                self.expression(depth + 1)?;
                self.source.push_str("))");
                Ok(())
            }
            5 => {
                self.source.push('{');
                let count = self.u.int_in_range(0..=4)?;
                for _ in 0..count {
                    match self.u.int_in_range(0..=2)? {
                        0 => {}
                        1 => self
                            .source
                            .push_str(&format!("k{} = ", self.u.int_in_range(0..=2)?)),
                        _ => {
                            self.source.push('[');
                            self.expression(depth + 1)?;
                            self.source.push_str("] = ");
                        }
                    }
                    self.expression(depth + 1)?;
                    self.source.push_str(", ");
                }
                self.source.push('}');
                Ok(())
            }
            6 if self.function > 0 => {
                let callee = self.u.int_in_range(0..=self.function - 1)?;
                self.source.push_str(&format!("f{callee}("));
                self.arguments(depth + 1)?;
                self.source.push(')');
                Ok(())
            }
            7 => {
                self.variable()?;
                self.source.push('[');
                self.expression(depth + 1)?;
                self.source.push(']');
                Ok(())
            }
            8 => self.library_call(depth + 1),
            _ => {
                self.source.push_str("(function(a, b) return ");
                self.expression(depth + 1)?;
                self.source.push_str(" end)");
                Ok(())
            }
        }
    }

    fn library_call(&mut self, depth: u32) -> Result<()> {
        const FUNCTIONS: [&str; 30] = [
            "tostring",
            "tonumber",
            "type",
            "select",
            "rawequal",
            "rawget",
            "math.floor",
            "math.abs",
            "math.max",
            "math.min",
            "math.fmod",
            "math.ceil",
            "string.len",
            "string.sub",
            "string.upper",
            "string.byte",
            "string.format",
            "string.find",
            "string.match",
            "string.gsub",
            "string.rep",
            "table.concat",
            "table.insert",
            "table.remove",
            "unpack",
            "bit.band",
            "bit.bor",
            "bit.bxor",
            "bit.lshift",
            "bit.tobit",
        ];
        let mut function = *self.u.choose(&FUNCTIONS)?;
        if self.dialect == Dialect::Deterministic {
            function = match function {
                "string.format" => "fmt",
                "table.concat" => "tconcat",
                "table.insert" => "tinsert",
                "table.remove" => "tremove",
                "unpack" => "tunpack",
                other => other,
            };
        }
        if function == "string.rep" {
            // Bounded, so that no program asks for more than any interpreter gives.
            self.source.push_str("string.rep(");
            self.expression(depth)?;
            self.source
                .push_str(&format!(", {})", self.u.int_in_range(0..=5)?));
            return Ok(());
        }
        self.source.push_str(&format!("{function}("));
        self.arguments(depth)?;
        self.source.push(')');
        Ok(())
    }

    fn atom(&mut self) -> Result<()> {
        match self.u.int_in_range(0..=7)? {
            0 => self.variable(),
            1 => {
                const NUMBERS: [&str; 14] = [
                    "0", "1", "-1", "2", "3", "10", "0.5", "-0", "1e300", "2^53", "0x10", "1/0",
                    "-1/0", "255",
                ];
                self.source.push_str(self.u.choose(&NUMBERS)?);
                Ok(())
            }
            2 => {
                const STRINGS: [&str; 12] = [
                    "\"\"",
                    "\"a\"",
                    "\"10\"",
                    "\" 5 \"",
                    "\"0x1A\"",
                    "\"1e2\"",
                    "\"abc\"",
                    "\"%d\"",
                    "\"%s=%5.2f\"",
                    "\"(%a+)\"",
                    "\"\\0z\"",
                    "\"a.b\"",
                ];
                self.source.push_str(self.u.choose(&STRINGS)?);
                Ok(())
            }
            3 => {
                self.source
                    .push_str(self.u.choose(&["nil", "true", "false"])?);
                Ok(())
            }
            4 if self.function < FUNCTIONS => {
                self.source.push_str(self.u.choose(&["a", "b"])?);
                Ok(())
            }
            5 => {
                self.source
                    .push_str(&format!("{}", self.u.int_in_range(-20..=300)?));
                Ok(())
            }
            6 => {
                self.variable()?;
                self.source
                    .push_str(&format!(".k{}", self.u.int_in_range(0..=2)?));
                Ok(())
            }
            _ => {
                self.source.push_str("v4");
                Ok(())
            }
        }
    }
}
