//! `debug`, reduced to what scripts use for logging: `traceback`, and `getinfo` with a function's source and lines.
//! Nothing that reads or changes another function's locals, upvalues, hooks or metatables.

use crate::heap::Function;
use crate::value::Value;
use crate::vm::{Args, LuaError, Vm};

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    vm.library("debug", &[("getinfo", getinfo), ("traceback", traceback)])?;
    Ok(())
}

fn traceback(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let message = vm.arg(args, 0);
    let mut text = match message {
        Value::String(s) => vm.heap.string(s).to_vec(),
        Value::Number(n) => crate::number::format(n).into_bytes(),
        Value::Nil => Vec::new(),
        other => return vm.results(&[other]),
    };
    if !text.is_empty() {
        text.push(b'\n');
    }
    text.extend_from_slice(b"stack traceback:");
    let level = vm.opt_integer(args, 1, 1)?;
    let frames = vm.traceback_text(usize::try_from(level).unwrap_or(0));
    text.extend_from_slice(&frames);
    vm.push_string(&text)
}

fn getinfo(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (function, line) = match vm.arg(args, 0) {
        Value::Function(f) => (Some(f), None),
        _ => {
            let level = vm.check_integer(args, 0)?;
            if level < 0 {
                return vm.results(&[Value::Nil]);
            }
            // Level 0 is getinfo itself, a native; level 1 the function that called it.
            match vm.frames.iter().rev().nth((level as usize).wrapping_sub(1)) {
                Some(frame) if level > 0 => {
                    let line = frame.proto.lines.get(frame.pc.saturating_sub(1)).copied();
                    (Some(frame.function), line)
                }
                _ if level == 0 => (None, None),
                _ => return vm.results(&[Value::Nil]),
            }
        }
    };
    let info = vm.heap.new_table(0, 6)?;
    let (source, what, defined) = match function.map(|f| vm.heap.functions.get(f.0)) {
        Some(Function::Lua { proto, .. }) => (
            format!("@{}", proto.chunk),
            if proto.line == 0 { "main" } else { "Lua" },
            f64::from(proto.line),
        ),
        _ => ("=[C]".to_owned(), "C", -1.0),
    };
    let short = source.trim_start_matches(['@', '=']).to_owned();
    let source = vm.new_string(source.as_bytes())?;
    vm.set_field(info, "source", source)?;
    let short = vm.new_string(short.as_bytes())?;
    vm.set_field(info, "short_src", short)?;
    let what = vm.new_string(what.as_bytes())?;
    vm.set_field(info, "what", what)?;
    vm.set_field(info, "linedefined", Value::Number(defined))?;
    vm.set_field(
        info,
        "currentline",
        Value::Number(line.map_or(-1.0, f64::from)),
    )?;
    if let Some(f) = function {
        vm.set_field(info, "func", Value::Function(f))?;
    }
    vm.results(&[Value::Table(info)])
}
