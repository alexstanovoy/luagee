//! A small capability table shared by embedding tests and coverage-guided inputs.

use std::cell::RefCell;
use std::rc::Rc;

use luagee::{Error, Lua, Value};

pub fn install(lua: &mut Lua) -> Result<(), Error> {
    let vm = lua.vm();
    // Installed without running script code; no collection can intervene during construction.
    let table = vm
        .new_table()
        .map_err(|_| Error::Limit(luagee::Limit::Memory))?;
    vm.pin(Value::Table(table))
        .map_err(|_| Error::Limit(luagee::Limit::Memory))?;
    let methods = callbacks();
    for (name, callback) in methods {
        let value = lua.function(name, callback)?;
        let vm = lua.vm();
        let key = vm
            .new_string(name.as_bytes())
            .map_err(|_| Error::Limit(luagee::Limit::Memory))?;
        vm.raw_set(table, key, value)
            .map_err(|_| Error::Limit(luagee::Limit::Memory))?;
    }
    lua.set_global("host", Value::Table(table))?;
    lua.vm()
        .unpin(Value::Table(table))
        .map_err(|_| Error::Limit(luagee::Limit::Memory))?;
    Ok(())
}

fn callbacks() -> Vec<(&'static str, luagee::HostFunction)> {
    let mut methods = argument_callbacks();
    methods.extend(object_callbacks());
    methods.extend(cache_callbacks());
    methods.extend(unrooted_callbacks());
    methods.extend(argument_scope_callbacks());
    methods
}

fn argument_callbacks() -> Vec<(&'static str, luagee::HostFunction)> {
    vec![
        (
            "arity",
            Rc::new(|vm, args| {
                vm.results(&[
                    Value::Number(args.len() as f64),
                    Value::Boolean(args.is_empty()),
                    vm.arg(args, args.len()),
                ])
            }),
        ),
        (
            "burn",
            Rc::new(|vm, _| {
                vm.charge_fuel(u64::MAX)?;
                Ok(0)
            }),
        ),
        (
            "string",
            Rc::new(|vm, args| {
                let value = vm.check_string(args, 0)?;
                let bytes = vm.bytes(Value::String(value)).unwrap_or_default();
                let size = bytes.len();
                vm.charge_fuel(1 + size as u64 / 16)?;
                let copy = vm.bytes(Value::String(value)).unwrap_or_default().to_vec();
                let string = vm.new_string(&copy)?;
                vm.results(&[string])
            }),
        ),
        (
            "make",
            Rc::new(|vm, args| {
                let value = vm.check_any(args, 0)?;
                let table = vm.new_table()?;
                vm.raw_set(table, Value::Number(1.0), value)?;
                vm.results(&[Value::Table(table)])
            }),
        ),
        (
            "read",
            Rc::new(|vm, args| {
                let table = vm.check_table(args, 0)?;
                let value = vm.raw_get(table, Value::Number(1.0))?;
                vm.results(&[value])
            }),
        ),
        (
            "convert",
            Rc::new(|vm, args| {
                let number = vm.check_integer(args, 0)?;
                vm.opt_integer(args, 1, 7)?;
                let string = vm.check_string(args, 2)?;
                let _bytes = vm.bytes(Value::String(string));
                vm.check_table(args, 3)?;
                vm.results(&[Value::Number(number as f64)])
            }),
        ),
        (
            "optional",
            Rc::new(|vm, args| {
                let number = vm.opt_integer(args, 0, 7)?;
                vm.opt_string(args, 1)?;
                vm.results(&[Value::Number(number as f64)])
            }),
        ),
    ]
}

fn object_callbacks() -> Vec<(&'static str, luagee::HostFunction)> {
    vec![
        (
            "object",
            Rc::new(|vm, args| {
                let number = vm.check_number(args, 0)?;
                let meta = vm.new_table()?;
                let key = vm.new_string(b"__metatable")?;
                let sealed = vm.new_string(b"sealed")?;
                vm.raw_set(meta, key, sealed)?;
                let value = vm.userdata(number, Some(meta), 0)?;
                vm.results(&[value])
            }),
        ),
        (
            "object_value",
            Rc::new(|vm, args| {
                let object = vm.arg(args, 0);
                let number = vm
                    .userdata_ref::<f64>(object)
                    .copied()
                    .ok_or_else(|| vm.arg_error(args, 0, "host object expected"))?;
                vm.results(&[Value::Number(number)])
            }),
        ),
        (
            "bad_key",
            Rc::new(|vm, args| {
                let table = vm.new_table()?;
                vm.raw_set(table, vm.arg(args, 0), Value::Boolean(true))?;
                Ok(0)
            }),
        ),
        (
            "invoke",
            Rc::new(|vm, args| {
                let function = vm.check_any(args, 0)?;
                vm.charge_fuel(1 + args.len() as u64 / 8)?;
                let arguments: Vec<Value> =
                    (1..args.len()).map(|index| vm.arg(args, index)).collect();
                let values = vm.call(function, &arguments)?;
                vm.results(&values)
            }),
        ),
        (
            "collect",
            Rc::new(|vm, args| {
                let function = vm.check_any(args, 0)?;
                let value = vm.check_any(args, 1)?;
                let table = vm.new_table()?;
                vm.pin(Value::Table(table))?;
                vm.raw_set(table, Value::Number(1.0), value)?;
                let result = vm.call(function, &[]);
                vm.unpin(Value::Table(table))?;
                result?;
                vm.results(&[Value::Table(table)])
            }),
        ),
    ]
}

fn cache_callbacks() -> Vec<(&'static str, luagee::HostFunction)> {
    let saved = Rc::new(RefCell::new(Vec::new()));
    vec![
        ("remember", {
            let saved = saved.clone();
            Rc::new(move |vm, args| {
                let value = vm.check_any(args, 0)?;
                vm.pin(value)?;
                saved.borrow_mut().push(value);
                Ok(0)
            })
        }),
        ("recall", {
            let saved = saved.clone();
            Rc::new(move |vm, _| {
                let value = saved.borrow().last().copied().unwrap_or(Value::Nil);
                vm.results(&[value])
            })
        }),
        ("forget", {
            let saved = saved.clone();
            Rc::new(move |vm, _| {
                while let Some(value) = saved.borrow_mut().pop() {
                    vm.unpin(value)?;
                }
                Ok(0)
            })
        }),
        (
            "retain",
            Rc::new(|vm, _| {
                let value = vm.userdata([0u8; 8], None, 4096)?;
                vm.pin(value)?;
                Ok(0)
            }),
        ),
    ]
}

fn unrooted_callbacks() -> Vec<(&'static str, luagee::HostFunction)> {
    let saved = Rc::new(std::cell::Cell::new(Value::Nil));
    vec![
        ("save_unrooted", {
            let saved = saved.clone();
            Rc::new(move |vm, args| {
                saved.set(vm.check_any(args, 0)?);
                Ok(0)
            })
        }),
        ("read_unrooted", {
            let saved = saved.clone();
            Rc::new(move |vm, _| {
                let value = vm.userdata_ref::<f64>(saved.get()).copied();
                vm.results(&[value.map_or(Value::Nil, Value::Number)])
            })
        }),
        ("result_unrooted", {
            let saved = saved.clone();
            Rc::new(move |vm, _| vm.results(&[saved.get()]))
        }),
        ("error_unrooted", Rc::new(move |_, _| {
            Err(luagee::LuaError::runtime(saved.get(), luagee::Traceback::default(), false))
        })),
    ]
}

fn argument_scope_callbacks() -> Vec<(&'static str, luagee::HostFunction)> {
    let saved = Rc::new(std::cell::Cell::new(None));
    vec![
        ("save_args", {
            let saved = saved.clone();
            Rc::new(move |_, args| { saved.set(Some(args)); Ok(0) })
        }),
        ("read_args", Rc::new(move |vm, _| {
            let args = saved.get().ok_or_else(|| vm.error("no saved callback arguments"))?;
            let value = vm.check_any(args, 0)?;
            vm.results(&[value])
        })),
        ("retain_results", Rc::new(|vm, args| {
            let values = vm.call(vm.arg(args, 0), &[])?;
            vm.call(vm.arg(args, 1), &[])?;
            vm.results(&values)
        })),
    ]
}
