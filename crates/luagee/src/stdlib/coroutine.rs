//! Coroutines: they cannot yield across a native call (`pcall`, a metamethod, `table.sort`'s comparator), as in Lua
//! 5.1.

use crate::value::Value;
use crate::vm::{Args, LuaError, ThreadStatus, Vm};

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    vm.library(
        "coroutine",
        &[
            ("create", create),
            ("isyieldable", isyieldable),
            ("resume", resume),
            ("running", running),
            ("status", status),
            ("wrap", wrap),
            ("yield", yield_),
        ],
    )?;
    Ok(())
}

fn create(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let function = vm.arg(args, 0);
    if !matches!(function, Value::Function(_)) {
        return Err(vm.arg_type_error(args, 0, "function"));
    }
    let thread = vm.new_thread(function)?;
    vm.results(&[Value::Thread(thread)])
}

fn resume(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let Value::Thread(co) = vm.arg(args, 0) else {
        return Err(vm.arg_error(args, 0, "coroutine expected"));
    };
    let values: Vec<Value> = (1..args.count).map(|i| vm.arg(args, i)).collect();
    match vm.resume(co, values)? {
        Ok(results) => {
            vm.push(Value::Boolean(true))?;
            for value in &results {
                vm.push(*value)?;
            }
            Ok(results.len() + 1)
        }
        Err(error) => vm.results(&[Value::Boolean(false), error]),
    }
}

fn wrap_step(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let Value::Thread(co) = vm.native_upvalue(args, 0) else {
        unreachable!("wrap keeps its coroutine")
    };
    let values: Vec<Value> = (0..args.count).map(|i| vm.arg(args, i)).collect();
    match vm.resume(co, values)? {
        Ok(results) => vm.results(&results),
        Err(error) => {
            // A string (or number) error gets the position of the wrapper's caller in front, as Lua 5.1's does.
            let position = vm.native_where(1);
            let value = match error {
                Value::String(_) if position.is_empty() => error,
                Value::String(s) => {
                    vm.charge_fuel(vm.heap.string(s).len() as u64 / 16)?;
                    let mut text = position;
                    text.extend_from_slice(vm.heap.string(s));
                    vm.new_string(&text)?
                }
                Value::Number(n) => {
                    let mut text = position;
                    text.extend_from_slice(crate::number::format(n).as_bytes());
                    vm.new_string(&text)?
                }
                _ => error,
            };
            Err(LuaError::Runtime {
                value,
                traceback: vm.traceback(),
                located: true,
            })
        }
    }
}

fn wrap(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let function = vm.arg(args, 0);
    if !matches!(function, Value::Function(_)) {
        return Err(vm.arg_type_error(args, 0, "function"));
    }
    let thread = vm.new_thread(function)?;
    let step = vm.native("wrap", wrap_step, vec![Value::Thread(thread)])?;
    vm.results(&[step])
}

fn yield_(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    if vm.current == vm.main {
        return Err(vm.error("attempt to yield from outside a coroutine"));
    }
    let depth = vm.threads.get(vm.current.0).depth;
    // The yield must come straight from the coroutine's own loop: one native level above its resume.
    if vm.native_depth != depth + 2 {
        return Err(vm.error("attempt to yield across a C-call boundary"));
    }
    vm.yielding = true;
    let values: Vec<Value> = (0..args.count).map(|i| vm.arg(args, i)).collect();
    vm.results(&values)
}

fn running(vm: &mut Vm, _args: Args) -> Result<usize, LuaError> {
    if vm.current == vm.main {
        return vm.results(&[Value::Nil]);
    }
    vm.results(&[Value::Thread(vm.current)])
}

fn isyieldable(vm: &mut Vm, _args: Args) -> Result<usize, LuaError> {
    let yieldable =
        vm.current != vm.main && vm.native_depth == vm.threads.get(vm.current.0).depth + 2;
    vm.results(&[Value::Boolean(yieldable)])
}

fn status(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let Value::Thread(co) = vm.arg(args, 0) else {
        return Err(vm.arg_error(args, 0, "coroutine expected"));
    };
    let text: &[u8] = match vm.threads.get(co.0).status {
        ThreadStatus::Fresh | ThreadStatus::Suspended => b"suspended",
        ThreadStatus::Running => b"running",
        ThreadStatus::Normal => b"normal",
        ThreadStatus::Dead => b"dead",
    };
    vm.push_string(text)
}
