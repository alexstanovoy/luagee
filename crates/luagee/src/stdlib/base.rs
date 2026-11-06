//! The base library: `print`, `type`, `pcall`, `setfenv`, `require`, ... Source-only loading, no files.

use crate::heap::Function;
use crate::table::InvalidKey;
use crate::value::{StrId, TableId, Value};
use crate::vm::{Args, LuaError, Vm};
use crate::{Limit, ModuleName};

/// The most values a library function returns or unpacks at once (Lua 5.1's C stack limit).
pub(crate) const MAX_RESULTS: usize = 8000;

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    let globals = vm.globals;
    for (name, f) in [
        ("assert", assert as super::Native),
        ("collectgarbage", collectgarbage),
        ("dofile", dofile),
        ("error", error),
        ("gcinfo", gcinfo),
        ("getfenv", getfenv),
        ("getmetatable", getmetatable),
        ("load", load),
        ("loadstring", loadstring),
        ("module", module),
        ("next", next),
        ("pairs", pairs),
        ("pcall", pcall),
        ("print", print),
        ("rawequal", rawequal),
        ("rawget", rawget),
        ("rawset", rawset),
        ("select", select),
        ("setfenv", setfenv),
        ("setmetatable", setmetatable),
        ("tonumber", tonumber),
        ("tostring", tostring),
        ("type", type_),
        ("unpack", unpack),
        ("xpcall", xpcall),
    ] {
        vm.register(globals, name, f)?;
    }
    let step = vm.native("ipairs_aux", ipairs_step, Vec::new())?;
    let ipairs = vm.native("ipairs", ipairs, vec![step])?;
    vm.set_field(globals, "ipairs", ipairs)?;
    vm.set_field(globals, "_G", Value::Table(globals))?;
    let version = vm.new_string(b"Lua 5.1")?;
    vm.set_field(globals, "_VERSION", version)?;
    let package = vm.heap.new_table(0, 4)?;
    vm.set_field(package, "loaded", Value::Table(vm.loaded))?;
    let preload = vm.heap.new_table(0, 0)?;
    vm.set_field(package, "preload", Value::Table(preload))?;
    vm.register(package, "seeall", seeall)?;
    vm.set_field(globals, "package", Value::Table(package))?;
    // require reads package.preload from this table, whatever the global `package` becomes, as in Lua.
    let require = vm.native("require", require, vec![Value::Table(package)])?;
    vm.set_field(globals, "require", require)?;
    vm.set_field(vm.loaded, "_G", Value::Table(globals))?;
    vm.set_field(vm.loaded, "package", Value::Table(package))?;
    Ok(())
}

fn assert(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let value = vm.check_any(args, 0)?;
    if value.is_truthy() {
        let values: Vec<Value> = (0..args.count).map(|i| vm.arg(args, i)).collect();
        return vm.results(&values);
    }
    let message = match vm.arg(args, 1) {
        Value::String(s) => vm.heap.string(s).to_vec(),
        // A number message is raised as its text, with a position, as LuaJIT does.
        Value::Number(n) => crate::number::format(n).into_bytes(),
        Value::Nil if args.count < 2 => b"assertion failed!".to_vec(),
        // Any other message is raised as it is, as LuaJIT does.
        other => {
            return Err(LuaError::Runtime {
                value: other,
                traceback: vm.traceback(),
                located: true,
            });
        }
    };
    let error = vm.error(message);
    Err(if vm.call_site.is_from_lua() {
        error
    } else {
        vm.locate_nowhere(error)
    })
}

fn collectgarbage(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let option = match vm.opt_string(args, 0)? {
        Some(s) => vm.heap.string(s).to_vec(),
        None => b"collect".to_vec(),
    };
    match option.as_slice() {
        b"collect" => {
            vm.request_collection();
            vm.results(&[Value::Number(0.0)])
        }
        b"step" => {
            vm.request_collection();
            vm.results(&[Value::Boolean(true)])
        }
        b"count" => {
            let kilobytes = crate::number::div(vm.heap.memory as f64, 1024.0);
            vm.results(&[Value::Number(kilobytes)])
        }
        // The collector runs by the interpreter's own accounting; scripts cannot stop or tune it.
        b"stop" | b"restart" => vm.results(&[Value::Number(0.0)]),
        b"setpause" | b"setstepmul" => vm.results(&[Value::Number(200.0)]),
        _ => {
            let message = format!("invalid option '{}'", crate::shown(&option));
            Err(vm.arg_error(args, 0, &message))
        }
    }
}

fn gcinfo(vm: &mut Vm, _args: Args) -> Result<usize, LuaError> {
    let kilobytes = (vm.heap.memory / 1024) as f64;
    vm.results(&[Value::Number(kilobytes)])
}

fn error(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let value = vm.arg(args, 0);
    let level = vm.opt_integer(args, 1, 1)?;
    // A number is a message too: raised as its text, with the position (Lua 5.1's `lua_isstring`). Level 1 is the
    // function that called `error`.
    let value = match value {
        Value::String(_) | Value::Number(_) if level > 0 => {
            let position = vm.native_where(level);
            match value {
                Value::String(_) if position.is_empty() => value,
                _ => {
                    let mut text = position;
                    match value {
                        Value::String(s) => {
                            vm.charge_fuel(vm.heap.string(s).len() as u64 / 16)?;
                            text.extend_from_slice(vm.heap.string(s));
                        }
                        Value::Number(n) => {
                            text.extend_from_slice(crate::number::format(n).as_bytes())
                        }
                        _ => unreachable!("a string or a number"),
                    }
                    vm.new_string(&text)?
                }
            }
        }
        _ => value,
    };
    Err(LuaError::Runtime {
        value,
        traceback: vm.traceback(),
        located: true,
    })
}

/// The function at a `getfenv`/`setfenv` level (1: the caller), or None for level 0 (the thread's globals).
fn function_at_level(vm: &mut Vm, args: Args, level: i64) -> Result<Option<Value>, LuaError> {
    if level < 0 {
        return Err(vm.arg_error(args, 0, "level must be non-negative"));
    }
    if level == 0 {
        return Ok(None);
    }
    match vm.frames.iter().rev().nth(level as usize - 1) {
        Some(frame) => Ok(Some(Value::Function(frame.function))),
        None => Err(vm.arg_error(args, 0, "invalid level")),
    }
}

fn target_function(vm: &mut Vm, args: Args) -> Result<Option<Value>, LuaError> {
    match vm.arg(args, 0) {
        f @ Value::Function(_) => Ok(Some(f)),
        Value::Nil => function_at_level(vm, args, 1),
        _ => {
            let level = vm.check_integer(args, 0)?;
            function_at_level(vm, args, level)
        }
    }
}

fn getfenv(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let env = match target_function(vm, args)? {
        Some(Value::Function(f)) => match vm.heap.functions.get(f.0) {
            Function::Lua { env, .. } => *env,
            Function::Native { .. } => vm.globals,
        },
        _ => vm.globals,
    };
    vm.results(&[Value::Table(env)])
}

/// Gives a Lua function a new environment; its running activations (in every coroutine) read it from the function.
pub(crate) fn set_env(vm: &mut Vm, f: crate::value::FuncId, table: TableId) -> bool {
    match vm.heap.functions.get_mut(f.0) {
        Function::Lua { env, .. } => *env = table,
        Function::Native { .. } => return false,
    }
    vm.heap.barrier(Value::Table(table));
    true
}

fn setfenv(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 1)?;
    match target_function(vm, args)? {
        None => {
            vm.globals = table;
            Ok(0)
        }
        Some(Value::Function(f)) => {
            if !set_env(vm, f, table) {
                return Err(vm.error("'setfenv' cannot change environment of given object"));
            }
            vm.results(&[Value::Function(f)])
        }
        Some(_) => unreachable!("a level names a function"),
    }
}

fn getmetatable(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let value = vm.check_any(args, 0)?;
    let Some(mt) = vm.metatable(value) else {
        return vm.results(&[Value::Nil]);
    };
    let protected = vm.heap.raw_get(mt, Value::String(vm.names.metatable));
    if protected != Value::Nil {
        return vm.results(&[protected]);
    }
    vm.results(&[Value::Table(mt)])
}

fn setmetatable(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let metatable = match vm.arg(args, 1) {
        Value::Nil if args.count >= 2 => None,
        Value::Table(mt) => Some(mt),
        _ => return Err(vm.arg_error(args, 1, "nil or table expected")),
    };
    if let Some(current) = vm.heap.table(table).metatable
        && vm.heap.raw_get(current, Value::String(vm.names.metatable)) != Value::Nil
    {
        return Err(vm.error("cannot change a protected metatable"));
    }
    vm.heap.set_metatable(table, metatable);
    vm.results(&[Value::Table(table)])
}

fn ipairs_step(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let i = vm.check_integer(args, 1)? + 1;
    let value = vm.heap.tables.get(table.0).get_int(i, &vm.heap.hasher);
    if value == Value::Nil {
        return vm.results(&[Value::Nil]);
    }
    vm.results(&[Value::Number(i as f64), value])
}

fn ipairs(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    vm.check_any(args, 0)?;
    let table = vm.check_table(args, 0)?;
    // The same iterator every time, as LuaJIT gives it.
    let step = vm.native_upvalue(args, 0);
    vm.results(&[step, Value::Table(table), Value::Number(0.0)])
}

fn next(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let key = vm.arg(args, 1);
    let hash = vm.heap.key_hash(key);
    match vm.heap.table(table).next(key, hash) {
        Ok((found, skipped)) => {
            vm.charge_fuel(skipped as u64)?;
            match found {
                Some((k, v)) => vm.results(&[k, v]),
                None => vm.results(&[Value::Nil]),
            }
        }
        Err(InvalidKey) => Err(vm.error("invalid key to 'next'")),
    }
}

fn pairs(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    // LuaJIT asks for any value first, then for a table.
    vm.check_any(args, 0)?;
    let table = vm.check_table(args, 0)?;
    let next_value = vm.get_field(vm.globals, "next")?;
    let next_value = match next_value {
        Value::Function(_) => next_value,
        _ => vm.native("next", next, Vec::new())?,
    };
    vm.results(&[next_value, Value::Table(table), Value::Nil])
}

/// Calls the function at `slot` protected, with a message handler (nil for none) run where an error happens:
/// Ok(results) or Err(the handled error); limits pass through.
pub(crate) fn protected_call(
    vm: &mut Vm,
    slot: usize,
    count: usize,
    handler: Value,
) -> Result<Result<usize, Value>, LuaError> {
    let called = vm.protect_with(handler, |vm| vm.call_value(slot, count))?;
    if called.is_err() {
        vm.stack.truncate(slot);
    }
    Ok(called)
}

fn pcall(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    vm.check_any(args, 0)?;
    let slot = args.base;
    match protected_call(vm, slot, args.count - 1, Value::Nil)? {
        Ok(n) => {
            vm.stack.insert(slot, Value::Boolean(true));
            Ok(n + 1)
        }
        Err(value) => vm.results(&[Value::Boolean(false), value]),
    }
}

fn xpcall(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let handler = vm.check_any(args, 1)?;
    let slot = args.base + args.count;
    vm.push(vm.arg(args, 0))?;
    for i in 2..args.count {
        vm.push(vm.arg(args, i))?;
    }
    match protected_call(vm, slot, args.count.saturating_sub(2), handler)? {
        Ok(n) => {
            vm.stack.insert(slot, Value::Boolean(true));
            Ok(n + 1)
        }
        Err(handled) => vm.results(&[Value::Boolean(false), handled]),
    }
}

fn print(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let mut line = Vec::new();
    for i in 0..args.count {
        if i > 0 {
            line.push(b'\t');
        }
        let shown = vm.display_value(vm.stack[args.base + i])?;
        let piece: std::rc::Rc<[u8]> = match shown {
            Value::String(s) => vm.heap.string_rc(s),
            Value::Number(n) => crate::number::format(n).as_bytes().into(),
            _ => return Err(vm.error("'tostring' must return a string to 'print'")),
        };
        // Paid and checked before the copy.
        vm.charge_fuel(1 + piece.len() as u64 / 16)?;
        if line.len() + piece.len() > vm.limits.string_length {
            return Err(vm.string_overflow());
        }
        line.extend_from_slice(&piece);
    }
    line.push(b'\n');
    if let Some(sink) = vm.print.as_mut() {
        sink(&line);
    }
    Ok(0)
}

fn rawequal(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let a = vm.check_any(args, 0)?;
    let b = vm.check_any(args, 1)?;
    vm.results(&[Value::Boolean(Vm::raw_equal(a, b))])
}

fn rawget(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let key = vm.check_any(args, 1)?;
    let value = vm.heap.raw_get(table, key);
    vm.results(&[value])
}

fn rawset(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let key = vm.check_any(args, 1)?;
    let value = vm.check_any(args, 2)?;
    vm.raw_set_checked(table, key, value)?;
    vm.results(&[Value::Table(table)])
}

fn select(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let rest = args.count.saturating_sub(1) as i64;
    if let Value::String(s) = vm.arg(args, 0)
        && vm.heap.string(s) == b"#"
    {
        return vm.results(&[Value::Number(rest as f64)]);
    }
    let n = vm.check_integer(args, 0)?;
    let first = if n < 0 { rest + n } else { n - 1 };
    if n == 0 || first < 0 {
        return Err(vm.arg_error(args, 0, "index out of range"));
    }
    let values: Vec<Value> = (first.min(rest) as usize..rest as usize)
        .map(|i| vm.stack[args.base + 1 + i])
        .collect();
    vm.results(&values)
}

fn tonumber(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let value = vm.check_any(args, 0)?;
    let base = vm.opt_integer(args, 1, 10)?;
    if base == 10 {
        let n = vm.coerce_number(value).map_or(Value::Nil, Value::Number);
        return vm.results(&[n]);
    }
    // LuaJIT asks for the string before it checks the base.
    let s = vm.check_string(args, 0)?;
    if !(2..=36).contains(&base) {
        return Err(vm.arg_error(args, 1, "base out of range"));
    }
    vm.charge_fuel(vm.heap.string(s).len() as u64 / 16)?;
    let text = vm.heap.string(s);
    let result = parse_in_base(text, base as u32).map_or(Value::Nil, Value::Number);
    vm.results(&[result])
}

/// `tonumber(s, base)`: an optional sign and `0x` (base 16), digits in the base, surrounding whitespace.
fn parse_in_base(text: &[u8], base: u32) -> Option<f64> {
    let text = std::str::from_utf8(text)
        .ok()?
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\u{b}');
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let digits = if base == 16 {
        digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
            .unwrap_or(digits)
    } else {
        digits
    };
    if digits.is_empty() {
        return None;
    }
    let mut value = 0u64;
    for c in digits.chars() {
        let digit = c.to_digit(base)?;
        value = value
            .saturating_mul(u64::from(base))
            .saturating_add(u64::from(digit));
    }
    let magnitude = value as f64;
    Some(if negative {
        crate::number::neg(magnitude)
    } else {
        magnitude
    })
}

fn tostring(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let value = vm.check_any(args, 0)?;
    let shown = vm.display_value(value)?;
    vm.results(&[shown])
}

fn type_(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let value = vm.check_any(args, 0)?;
    vm.push_string(value.type_name().as_bytes())
}

fn unpack(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let first = vm.opt_integer(args, 1, 1)?;
    let last = match vm.arg(args, 2) {
        Value::Nil => vm.heap.table(table).border() as i64,
        _ => vm.check_integer(args, 2)?,
    };
    if first > last {
        return Ok(0);
    }
    let count = (last as i128 - first as i128 + 1) as u128;
    if count >= MAX_RESULTS as u128 {
        return Err(vm.error("too many results to unpack"));
    }
    vm.charge_fuel(count as u64)?;
    for i in first..=last {
        let value = vm.heap.tables.get(table.0).get_int(i, &vm.heap.hasher);
        vm.push(value)?;
    }
    Ok(count as usize)
}

// --- loading -------------------------------------------------------------------------------------------------

/// Compiles source into a function in the thread's globals, counting it against the source limits. Ok(Err(message))
/// for source that does not compile. `name` is the chunk's name as given (what `debug.getinfo` reports as its
/// source), `chunk` that name as positions print it.
pub(crate) fn compile(
    vm: &mut Vm,
    source: &[u8],
    name: StrId,
    chunk: &str,
) -> Result<Result<Value, String>, LuaError> {
    if source.first() == Some(&0x1b) {
        return Ok(Err(format!(
            "{chunk}: cannot load bytecode: only source text is accepted"
        )));
    }
    if source.len() > vm.limits.module_size {
        return Err(LuaError::Limit(Limit::ModuleSize));
    }
    vm.scripts_loaded += source.len();
    if vm.scripts_loaded > vm.limits.scripts_size {
        return Err(LuaError::Limit(Limit::ScriptsSize));
    }
    vm.modules_loaded += 1;
    if vm.modules_loaded > vm.limits.module_count {
        return Err(LuaError::Limit(Limit::ModuleCount));
    }
    vm.charge_fuel(source.len() as u64 / 4)?;
    crate::lua::compile_chunk(vm, source, name, chunk)
}

/// Lua's chunk names as LuaJIT shortens them (`lj_debug_shortname`, with LuaJIT's LUA_IDSIZE of 60): `=name` as is,
/// `@file` as the file (its last part when long), anything else as `[string "first line..."]`.
pub(crate) fn chunk_id(name: &[u8]) -> String {
    const ID_SIZE: usize = 60;
    // C's strings end at the first NUL.
    let c_string = |bytes: &[u8]| -> String {
        let end = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    };
    match name.first() {
        Some(b'=') => c_string(&name[1..name.len().min(ID_SIZE)]),
        Some(b'@') if name.len() > ID_SIZE => {
            format!("...{}", c_string(&name[name.len() - (ID_SIZE - 4)..]))
        }
        Some(b'@') => c_string(&name[1..]),
        _ => {
            // Up to the first control character, and shorter still when that is not the end.
            let len = name
                .iter()
                .take(ID_SIZE - 12)
                .take_while(|&&c| c >= b' ')
                .count();
            if name.get(len).is_none_or(|&c| c == 0) {
                format!("[string \"{}\"]", c_string(&name[..len]))
            } else {
                format!(
                    "[string \"{}...\"]",
                    c_string(&name[..len.min(ID_SIZE - 15)])
                )
            }
        }
    }
}

fn loaded_result(vm: &mut Vm, compiled: Result<Value, String>) -> Result<usize, LuaError> {
    match compiled {
        Ok(function) => vm.results(&[function]),
        Err(message) => {
            let message = vm.new_string(message.as_bytes())?;
            vm.results(&[Value::Nil, message])
        }
    }
}

fn loadstring(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    // LuaJIT's loadstring is its load: anything but a string is asked to be a reader function.
    if !matches!(vm.arg(args, 0), Value::String(_) | Value::Number(_)) {
        return load(vm, args);
    }
    let text = vm.check_string(args, 0)?;
    // Without a name, the chunk is named by its source, as in Lua.
    let name = vm.opt_string(args, 1)?.unwrap_or(text);
    let chunk = chunk_id(vm.heap.string(name));
    let source = vm.heap.string(text).to_vec();
    let compiled = compile(vm, &source, name, &chunk)?;
    loaded_result(vm, compiled)
}

fn load(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let chunk = vm.arg(args, 0);
    let given = vm.opt_string(args, 1)?;
    let source = match chunk {
        Value::String(s) => vm.heap.string(s).to_vec(),
        Value::Function(_) => {
            let mut source = Vec::new();
            loop {
                // An error in the reader is load's failure, as in LuaJIT: nil and the error.
                let piece = match vm.protect(|vm| vm.call_meta(chunk, &[]))? {
                    Ok(piece) => piece,
                    Err(error) => return vm.results(&[Value::Nil, error]),
                };
                match piece {
                    Value::Nil => break,
                    Value::String(s) => {
                        let bytes = vm.heap.string(s);
                        if bytes.is_empty() {
                            break;
                        }
                        if source.len() + bytes.len() > vm.limits.module_size {
                            return Err(LuaError::Limit(Limit::ModuleSize));
                        }
                        source.extend_from_slice(bytes);
                    }
                    _ => {
                        let mut message = vm.where_(1);
                        message.extend_from_slice(b"reader function must return a string");
                        let message = vm.new_string(&message)?;
                        return vm.results(&[Value::Nil, message]);
                    }
                }
            }
            source
        }
        _ => return Err(vm.arg_type_error(args, 0, "function")),
    };
    // Without a name, a string chunk is named by its source, as in Lua, and a reader's chunk is "=(load)".
    let name = match (given, chunk) {
        (Some(name), _) => name,
        (None, Value::String(text)) => text,
        (None, _) => vm.heap.intern(b"=(load)")?,
    };
    let shown = chunk_id(vm.heap.string(name));
    let compiled = compile(vm, &source, name, &shown)?;
    loaded_result(vm, compiled)
}

/// Reads a module through the host's loader: the module name checked to stay in the script folder.
fn read_module(vm: &mut Vm, args: Args, name: &[u8]) -> Result<Vec<u8>, LuaError> {
    let text = crate::shown(name);
    let module: ModuleName = match text.parse() {
        Ok(module) => module,
        Err(e) => return Err(vm.error(format!("bad module name '{text}': {e}"))),
    };
    let _ = args;
    let source = match vm.loader.as_mut() {
        Some(loader) => loader.load(&module),
        None => None,
    };
    source.ok_or_else(|| vm.error(format!("module '{text}' not found")))
}

fn run_module(vm: &mut Vm, args: Args, name: &[u8], arg: Option<Value>) -> Result<usize, LuaError> {
    let source = read_module(vm, args, name)?;
    let chunk = format!("@{}", crate::shown(name));
    let source_name = vm.heap.intern(chunk.as_bytes())?;
    let function = match compile(vm, &source, source_name, &chunk_id(chunk.as_bytes()))? {
        Ok(function) => function,
        Err(message) => {
            return Err(vm.error(format!(
                "error loading module '{}':\n\t{message}",
                crate::shown(name)
            )));
        }
    };
    let slot = vm.stack.len();
    vm.push(function)?;
    let count = match arg {
        Some(value) => {
            vm.push(value)?;
            1
        }
        None => 0,
    };
    let produced = vm.call_value(slot, count)?;
    Ok(produced)
}

/// A module name argument, refused at once when it is too long to be one (so no work scales with a huge string).
fn module_argument(vm: &mut Vm, args: Args) -> Result<crate::value::StrId, LuaError> {
    let name = vm.check_string(args, 0)?;
    let length = vm.heap.string(name).len();
    vm.charge_fuel(1 + length as u64 / 16)?;
    if length > ModuleName::MAX_LENGTH {
        let shown = crate::shown(vm.heap.string(name));
        return Err(vm.error(format!(
            "bad module name '{shown}': {}",
            crate::ModuleNameError::TooLong
        )));
    }
    Ok(name)
}

fn dofile(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let name = module_argument(vm, args)?;
    let name = vm.heap.string(name).to_vec();
    let slot = vm.stack.len();
    let produced = run_module(vm, args, &name, None)?;
    let values = vm.stack[slot..slot + produced].to_vec();
    vm.stack.truncate(slot);
    vm.results(&values)
}

/// The loader `package.preload` has for a module, the first place Lua's require looks.
fn preloader(vm: &mut Vm, args: Args, name: StrId) -> Result<Option<Value>, LuaError> {
    let Value::Table(package) = vm.native_upvalue(args, 0) else {
        unreachable!("require is made with the package table")
    };
    match vm.get_field(package, "preload")? {
        Value::Table(preload) => Ok(match vm.heap.raw_get(preload, Value::String(name)) {
            Value::Nil => None,
            loader => Some(loader),
        }),
        _ => Err(vm.error("'package.preload' must be a table")),
    }
}

fn require(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let name = module_argument(vm, args)?;
    let loaded = vm.heap.raw_get(vm.loaded, Value::String(name));
    if loaded.is_truthy() {
        if loaded == Value::Table(vm.loaded) {
            let text = crate::shown(vm.heap.string(name));
            return Err(vm.error(format!("loop or previous error loading module '{text}'")));
        }
        return vm.results(&[loaded]);
    }
    let preloaded = preloader(vm, args, name)?;
    // While a module runs, its entry is a sentinel (the loaded table itself), so a cycle is an error.
    vm.heap
        .raw_set(vm.loaded, Value::String(name), Value::Table(vm.loaded))?;
    let result = match preloaded {
        Some(loader) => vm.call_meta(loader, &[Value::String(name)]),
        None => {
            let path: Vec<u8> = vm
                .heap
                .string(name)
                .iter()
                .map(|&c| if c == b'.' { b'/' } else { c })
                .collect();
            let slot = vm.stack.len();
            run_module(vm, args, &path, Some(Value::String(name))).map(|produced| {
                let result = if produced > 0 {
                    vm.stack[slot]
                } else {
                    Value::Nil
                };
                vm.stack.truncate(slot);
                result
            })
        }
    };
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            vm.heap
                .raw_set(vm.loaded, Value::String(name), Value::Nil)?;
            return Err(e);
        }
    };
    let current = vm.heap.raw_get(vm.loaded, Value::String(name));
    let value = if result != Value::Nil {
        result
    } else if current != Value::Table(vm.loaded) {
        current
    } else {
        Value::Boolean(true)
    };
    vm.heap.raw_set(vm.loaded, Value::String(name), value)?;
    vm.results(&[value])
}

fn module(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let name = vm.check_string(args, 0)?;
    vm.charge_fuel(1 + vm.heap.string(name).len() as u64 / 16)?;
    let name_bytes = vm.heap.string(name).to_vec();
    let existing = vm.heap.raw_get(vm.loaded, Value::String(name));
    let table = match existing {
        // The loaded table itself is require's mark of a module still loading, not a module.
        Value::Table(t) if t != vm.loaded => t,
        _ => {
            // Find or make the nested tables of `a.b.c` in the globals.
            let mut table = vm.globals;
            for part in name_bytes.split(|&c| c == b'.') {
                let key = vm.new_string(part)?;
                table = match vm.heap.raw_get(table, key) {
                    Value::Table(t) => t,
                    Value::Nil => {
                        let t = vm.heap.new_table(0, 4)?;
                        vm.heap.raw_set(table, key, Value::Table(t))?;
                        t
                    }
                    _ => {
                        return Err(vm.error(format!(
                            "name conflict for module '{}'",
                            crate::shown(&name_bytes)
                        )));
                    }
                };
            }
            vm.heap
                .raw_set(vm.loaded, Value::String(name), Value::Table(table))?;
            table
        }
    };
    if vm.get_field(table, "_NAME")? == Value::Nil {
        vm.set_field(table, "_M", Value::Table(table))?;
        vm.set_field(table, "_NAME", Value::String(name))?;
        let package = match name_bytes.iter().rposition(|&c| c == b'.') {
            Some(dot) => &name_bytes[..=dot],
            None => &[][..],
        };
        let package = vm.new_string(package)?;
        vm.set_field(table, "_PACKAGE", package)?;
    }
    if let Some(frame) = vm.frames.last() {
        let f = frame.function;
        set_env(vm, f, table);
    }
    for i in 1..args.count {
        let option = vm.stack[args.base + i];
        vm.call_meta(option, &[Value::Table(table)])?;
    }
    Ok(0)
}

fn seeall(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let table = vm.check_table(args, 0)?;
    let meta = match vm.heap.table(table).metatable {
        Some(mt) => mt,
        None => {
            let mt = vm.heap.new_table(0, 1)?;
            vm.heap.set_metatable(table, Some(mt));
            mt
        }
    };
    let globals = vm.globals;
    vm.heap
        .raw_set(meta, Value::String(vm.names.index), Value::Table(globals))?;
    Ok(0)
}
