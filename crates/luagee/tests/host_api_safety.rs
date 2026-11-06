//! Security regressions at the public embedding boundary.

use std::cell::Cell;
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::panic::{AssertUnwindSafe, catch_unwind};

use luagee::{Args, Error, GcStress, Keys, Limits, Lua, Value};

fn interpreter() -> Lua {
    Lua::new(
        Limits {
            fuel_per_call: 20_000,
            fuel_per_tick: 40_000,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [17, 19],
            random: 23,
        },
    )
    .expect("interpreter")
}

fn register(lua: &mut Lua, name: &'static str, callback: luagee::HostFunction) {
    let value = lua.function(name, callback).expect("callback");
    lua.set_global(name, value).expect("global");
}

#[cfg(not(test))]
pub fn contracts() {
    #[cfg(not(target_arch = "wasm32"))]
    payload_drop_panic_during_script_gc_retires();
    #[cfg(not(target_arch = "wasm32"))]
    payload_drop_panic_inside_nested_vm_call_caught_by_the_callback_retires();
    top_level_vm_calls_cannot_bypass_tick_accounting();
    host_table_writes_have_linear_cost_for_a_growing_list();
    saved_argument_contexts_cannot_read_a_later_callback();
    nested_results_remain_live_across_reentry_without_explicit_pins();
    typed_userdata_mutation_rejects_other_types_and_stale_handles();
    hash_churn_pays_for_rebuilding_without_storage_growth();
    mixed_hash_and_array_writes_stay_linear();
    interrupted_migration_leaves_the_original_table_valid();
    require_retries_after_a_module_exhausts_fuel();
    require_retries_after_fuel_exhaustion_during_cache_commit();
    foreign_handles_are_rejected_even_with_identical_keys();
    argument_contexts_cannot_cross_interpreters();
    top_level_vm_setup_uses_a_fresh_budget();
    callback_loop_results_remain_charged_until_return();
    #[cfg(not(target_arch = "wasm32"))]
    callback_panic_does_not_leave_an_active_callback_context();
    #[cfg(not(target_arch = "wasm32"))]
    catching_a_nested_panic_cannot_resume_the_interpreter();
    #[cfg(not(target_arch = "wasm32"))]
    a_payload_drop_panic_retires_explicit_collection();
}

#[cfg_attr(test, test)]
fn top_level_vm_calls_cannot_bypass_tick_accounting() {
    let mut lua = interpreter();
    let function = lua.load("spin", b"for i=1,100 do end").expect("function");
    lua.pin(function).expect("pin");
    lua.begin_tick();
    let before = lua.tick_fuel_left();
    let ended = lua.vm().call(function, &[]);
    assert!(
        matches!(ended, Err(luagee::LuaError::Runtime(_))),
        "{ended:?}"
    );
    assert_eq!(lua.tick_fuel_left(), before);
    lua.call(function, &[]).expect("top level uses Lua::call");
    assert!(lua.tick_fuel_left() < before);
}

#[cfg_attr(test, test)]
fn saved_argument_contexts_cannot_read_a_later_callback() {
    let mut lua = interpreter();
    let saved = Rc::new(Cell::<Option<Args>>::new(None));
    let capture = saved.clone();
    register(
        &mut lua,
        "save",
        Rc::new(move |_, args| {
            capture.set(Some(args));
            Ok(0)
        }),
    );
    register(
        &mut lua,
        "read",
        Rc::new(move |vm, _| {
            let args = saved.get().expect("the script saved an argument context");
            assert_eq!(vm.arg(args, 0), Value::Nil);
            assert!(vm.opt_integer(args, 0, 0).is_err());
            assert!(vm.opt_string(args, 0).is_err());
            assert!(vm.check_table(args, 0).is_err());
            let value = vm.check_any(args, 0)?;
            vm.results(&[value])
        }),
    );
    let ended = lua.exec("expired", b"save(17) return read(29)");
    assert!(
        matches!(ended, Err(Error::Runtime { message, .. }) if message.contains("expired callback arguments"))
    );
    lua.exec("after_expired", b"return 7")
        .expect("interpreter recovers");
    register(
        &mut lua,
        "outer",
        Rc::new(|vm, args| {
            let function = vm.check_any(args, 0)?;
            vm.call(function, &[])?;
            let value = vm.check_any(args, 1)?;
            vm.results(&[value])
        }),
    );
    lua.exec(
        "coroutine_scope",
        br#"
        assert(outer(function()
            local co = coroutine.create(function()
                save(17)
                coroutine.yield()
                read(29)
            end)
            assert(coroutine.resume(co))
            local ok, message = coroutine.resume(co)
            assert(not ok and message:find('expired callback arguments', 1, true))
        end, 71) == 71)
    "#,
    )
    .expect("coroutines restore only their enclosing argument context");
}

#[cfg_attr(test, test)]
fn nested_results_remain_live_across_reentry_without_explicit_pins() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter();
        register(
            &mut lua,
            "retain_results",
            Rc::new(|vm, args| {
                let values = vm.call(vm.arg(args, 0), &[])?;
                vm.call(vm.arg(args, 1), &[])?;
                vm.results(&values)
            }),
        );
        lua.set_gc_stress(stress);
        lua.exec("nested_results", b"local t = retain_results(function() return {value = 71} end, function() collectgarbage() end) assert(t.value == 71)").expect("nested results are callback-local roots");
    }
}

#[cfg_attr(test, test)]
fn host_table_writes_have_linear_cost_for_a_growing_list() {
    let mut lua = interpreter();
    register(
        &mut lua,
        "fill",
        Rc::new(|vm, _| {
            let table = vm.new_table()?;
            for index in 1..=1000 {
                vm.raw_set(
                    table,
                    Value::Number(f64::from(index)),
                    Value::Number(f64::from(index)),
                )?;
            }
            for index in 1..=1000 {
                vm.raw_set(table, Value::Number(1.0), Value::Number(f64::from(index)))?;
            }
            vm.results(&[Value::Table(table)])
        }),
    );
    lua.exec(
        "list",
        b"local t = fill() assert(#t == 1000 and t[1000] == 1000 and t[1] == 1000)",
    )
    .expect("linear writes fit the call budget");
}

#[cfg_attr(test, test)]
fn typed_userdata_mutation_rejects_other_types_and_stale_handles() {
    let mut lua = interpreter();
    let object = lua.vm().userdata(17u64, None, 0).expect("object");
    lua.pin(object).expect("root");
    *lua.vm()
        .userdata_mut::<u64>(object)
        .expect("typed mutation") = 29;
    assert_eq!(lua.userdata_ref::<u64>(object), Some(&29));
    assert!(lua.vm().userdata_mut::<f64>(object).is_none());
    *lua.userdata_mut::<u64>(object).expect("top-level mutation") = 41;
    assert_eq!(lua.vm().userdata_ref::<u64>(object), Some(&41));
    lua.unpin(object).expect("release");
    lua.collect().expect("collection");
    lua.begin_tick();
    let replacement = lua.vm().userdata(71u64, None, 0).expect("replacement");
    assert!(lua.vm().userdata_mut::<u64>(object).is_none());
    assert_eq!(lua.vm().userdata_ref::<u64>(replacement), Some(&71));
}

#[cfg_attr(test, test)]
fn hash_churn_pays_for_rebuilding_without_storage_growth() {
    let mut lua = interpreter();
    register(
        &mut lua,
        "churn",
        Rc::new(|vm, _| {
            let table = vm.new_table()?;
            for index in 0..1024 {
                vm.raw_set(
                    table,
                    Value::Number(f64::from(100_000 + index)),
                    Value::Boolean(true),
                )?;
            }
            for index in 0..512 {
                vm.raw_set(table, Value::Number(f64::from(100_000 + index)), Value::Nil)?;
                vm.raw_set(
                    table,
                    Value::Number(f64::from(200_000 + index)),
                    Value::Boolean(true),
                )?;
            }
            Ok(0)
        }),
    );
    let ended = lua.exec("hash_churn", b"pcall(churn)");
    assert!(
        matches!(ended, Err(Error::Limit(luagee::Limit::FuelPerCall))),
        "{ended:?}"
    );
}

#[cfg_attr(test, test)]
fn mixed_hash_and_array_writes_stay_linear() {
    let mut lua = interpreter();
    register(
        &mut lua,
        "migrate",
        Rc::new(|vm, _| {
            let table = vm.new_table()?;
            for index in (2..=2000).step_by(2) {
                vm.raw_set(
                    table,
                    Value::Number(f64::from(index)),
                    Value::Number(f64::from(index)),
                )?;
            }
            for index in (1..=1999).step_by(2) {
                vm.raw_set(
                    table,
                    Value::Number(f64::from(index)),
                    Value::Number(f64::from(index)),
                )?;
            }
            vm.results(&[Value::Table(table)])
        }),
    );
    lua.exec(
        "migration",
        b"local t = migrate() assert(#t == 2000 and t[2000] == 2000)",
    )
    .expect("migration pays for actual elements");
}

#[cfg_attr(test, test)]
fn interrupted_migration_leaves_the_original_table_valid() {
    let mut lua = Lua::new(
        Limits {
            fuel_per_call: 50_000,
            fuel_per_tick: 100_000,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [17, 19],
            random: 23,
        },
    )
    .expect("interpreter");
    register(
        &mut lua,
        "blocked",
        Rc::new(|vm, args| {
            let table = vm.check_table(args, 0)?;
            vm.charge_fuel(49_500)?;
            vm.raw_set(table, Value::Number(1.0), Value::Number(1.0))?;
            Ok(0)
        }),
    );
    lua.exec("setup", b"t={} for i=2,1000 do t[i]=i end")
        .expect("hash run");
    lua.begin_tick();
    let ended = lua.exec("blocked_migration", b"pcall(blocked,t)");
    assert!(
        matches!(ended, Err(Error::Limit(luagee::Limit::FuelPerCall))),
        "{ended:?}"
    );
    lua.begin_tick();
    lua.exec(
        "retry",
        b"assert(t[1] == nil and t[1000] == 1000) t[1] = 1 assert(#t == 1000)",
    )
    .expect("failed migration did not mutate the array prefix");
}

#[cfg_attr(test, test)]
fn require_retries_after_a_module_exhausts_fuel() {
    let mut lua = interpreter();
    let exhausted = Rc::new(Cell::new(true));
    let state = exhausted.clone();
    register(
        &mut lua,
        "module_value",
        Rc::new(move |vm, _| {
            if state.get() {
                vm.charge_fuel(u64::MAX)?;
            }
            vm.results(&[Value::Number(71.0)])
        }),
    );
    lua.exec(
        "preload",
        b"package.preload.retry = function() return module_value() end",
    )
    .expect("preloaded module");
    let ended = lua.exec("exhaust", b"pcall(require, 'retry')");
    assert!(
        matches!(ended, Err(Error::Limit(luagee::Limit::FuelPerCall))),
        "{ended:?}"
    );
    exhausted.set(false);
    lua.begin_tick();
    lua.exec("retry", b"assert(require('retry') == 71)")
        .expect("the failed load left no sentinel");
}

#[cfg_attr(test, test)]
fn require_retries_after_fuel_exhaustion_during_cache_commit() {
    let mut observed_edge = false;
    // Find the exact exhausted-commit boundary without hard-coding VM instruction counts.
    for fuel in 1..=128 {
        let mut lua = Lua::new(
            Limits {
                fuel_per_call: fuel,
                fuel_per_tick: 1000,
                ..Limits::DEFAULT
            },
            Keys {
                hash: [17, 19],
                random: 23,
            },
        )
        .expect("interpreter");
        let drain = Rc::new(Cell::new(true));
        let succeeded = Rc::new(Cell::new(false));
        let phase = drain.clone();
        let finished = succeeded.clone();
        register(
            &mut lua,
            "loader",
            Rc::new(move |vm, _| {
                if phase.get() {
                    vm.charge_fuel(20)?;
                    finished.set(true);
                    Ok(0)
                } else {
                    vm.results(&[Value::Number(71.0)])
                }
            }),
        );
        if lua
            .exec("preload", b"package.preload.retry = loader")
            .is_err()
        {
            continue;
        }
        lua.begin_tick();
        let ended = lua.exec("commit", b"return require('retry')");
        if !succeeded.get() || !matches!(ended, Err(Error::Limit(luagee::Limit::FuelPerCall))) {
            continue;
        }
        observed_edge = true;
        drain.set(false);
        lua.begin_tick();
        let values = lua
            .exec("retry", b"return require('retry')")
            .expect("failed cache commit cleared its sentinel");
        assert_eq!(values, vec![Value::Number(71.0)]);
        break;
    }
    assert!(
        observed_edge,
        "the test reached a successful loader with an exhausted cache commit"
    );
}

#[cfg_attr(test, test)]
fn foreign_handles_are_rejected_even_with_identical_keys() {
    let mut source = interpreter();
    let mut target = interpreter();
    let foreign = source
        .vm()
        .userdata(17u64, None, 0)
        .expect("source payload");
    let own = target
        .vm()
        .userdata(29u64, None, 0)
        .expect("target payload");
    source.pin(foreign).expect("source root");
    target.pin(own).expect("target root");
    assert_eq!(target.userdata_ref::<u64>(foreign), None);
    assert!(target.userdata_mut::<u64>(foreign).is_none());
    assert!(target.pin(foreign).is_err());
    assert!(target.set_global("foreign", foreign).is_err());
    let foreign_string = source.string(b"source bytes").expect("source string");
    let _own_string = target.string(b"target bytes").expect("target string");
    assert!(target.bytes(foreign_string).is_none());
    let foreign_table = source.vm().new_table().expect("source table");
    let own_table = target.vm().new_table().expect("target table");
    assert!(
        target
            .vm()
            .raw_get(foreign_table, Value::Number(1.0))
            .is_err()
    );
    assert!(
        target
            .vm()
            .raw_set(foreign_table, Value::Number(1.0), own)
            .is_err()
    );
    assert!(
        target
            .vm()
            .raw_set(own_table, Value::Number(1.0), foreign)
            .is_err()
    );
    assert!(
        target
            .vm()
            .set_metatable(own_table, Some(foreign_table))
            .is_err()
    );
    let foreign_function = source
        .load("foreign", b"return ...")
        .expect("source function");
    let own_function = target.load("own", b"return ...").expect("target function");
    source.pin(foreign_function).expect("source root");
    target.pin(own_function).expect("target root");
    assert!(target.call(foreign_function, &[]).is_err());
    assert!(target.call(own_function, &[foreign]).is_err());
    assert_eq!(target.userdata_ref::<u64>(own), Some(&29));
}

#[cfg_attr(test, test)]
fn argument_contexts_cannot_cross_interpreters() {
    let mut source = interpreter();
    let mut target = interpreter();
    let saved = Rc::new(Cell::new(None));
    let capture = saved.clone();
    register(
        &mut source,
        "capture",
        Rc::new(move |_, args| {
            capture.set(Some(args));
            Ok(0)
        }),
    );
    register(
        &mut target,
        "capture",
        Rc::new(move |vm, _| {
            let args = saved.get().expect("source context");
            let value = vm.check_any(args, 0)?;
            vm.results(&[value])
        }),
    );
    source
        .exec("source", b"return capture(17)")
        .expect("source call");
    let ended = target.exec("target", b"return capture(29)");
    assert!(
        matches!(ended, Err(Error::Runtime { message, .. }) if message.contains("expired callback arguments"))
    );
}

#[cfg_attr(test, test)]
fn top_level_vm_setup_uses_a_fresh_budget() {
    for operation in 0..3 {
        let mut lua = interpreter();
        let table = lua.vm().new_table().expect("table");
        lua.pin(Value::Table(table)).expect("root");
        let ended = lua.exec("exhaust", b"while true do end");
        assert!(matches!(
            ended,
            Err(Error::Limit(luagee::Limit::FuelPerCall))
        ));
        let fuel = lua.tick_fuel_left();
        match operation {
            0 => {
                lua.vm().new_table().expect("fresh setup allocation");
            }
            1 => {
                lua.vm().pin(Value::Table(table)).expect("fresh setup root");
            }
            _ => {
                lua.vm()
                    .raw_set(table, Value::Number(1.0), Value::Number(7.0))
                    .expect("fresh setup write");
            }
        }
        assert_eq!(
            lua.tick_fuel_left(),
            fuel,
            "trusted setup does not execute script fuel"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, test)]
fn callback_panic_does_not_leave_an_active_callback_context() {
    for source in [
        &b"host_panic()"[..],
        &b"co = coroutine.create(function() host_panic() end) coroutine.resume(co)"[..],
    ] {
        let mut lua = interpreter();
        register(
            &mut lua,
            "host_panic",
            Rc::new(|_, _| panic!("host callback failure")),
        );
        let probe = lua
            .load("probe", b"return coroutine.running()")
            .expect("probe");
        lua.pin(probe).expect("root");
        let ended =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lua.exec("panic", source)));
        assert!(ended.is_err());
        assert!(matches!(
            lua.vm().call(probe, &[]),
            Err(luagee::LuaError::Runtime(_))
        ));
        lua.begin_tick();
        let ended = lua.call(probe, &[]);
        assert!(
            matches!(ended, Err(Error::Runtime { message, .. }) if message.contains("interpreter retired"))
        );
        assert!(
            lua.collect().is_err(),
            "retired collector state cannot be reused"
        );
    }
}

#[cfg_attr(test, test)]
fn callback_loop_results_remain_charged_until_return() {
    let mut lua = Lua::new(
        Limits {
            memory: 256 << 10,
            fuel_per_call: 50_000_000,
            fuel_per_tick: 50_000_000,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [17, 19],
            random: 23,
        },
    )
    .expect("interpreter");
    register(
        &mut lua,
        "retain_loop",
        Rc::new(|vm, args| {
            let make = vm.check_any(args, 0)?;
            for _ in 0..20_000 {
                drop(vm.call(make, &[])?);
            }
            Ok(0)
        }),
    );
    lua.set_gc_stress(GcStress::Incremental);
    let ended = lua.exec("loop_roots", b"retain_loop(function() return {} end)");
    assert!(
        matches!(ended, Err(Error::Limit(luagee::Limit::Memory))),
        "{ended:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, test)]
fn catching_a_nested_panic_cannot_resume_the_interpreter() {
    for source in [
        &b"swallow(function() host_panic() end) return 71"[..],
        &b"swallow(function() local co=coroutine.create(host_panic) coroutine.resume(co) end) return 71"[..],
        &b"local a,b,c=1,2,3 pcall(swallow,function() host_panic() end) return a+b+c"[..],
        &b"local a,b,c=1,2,3 pcall(swallow,function() coroutine.resume(coroutine.create(host_panic)) end) return a+b+c"[..],
        &b"local a,b,c=1,2,3 xpcall(function() swallow(function() host_panic() end) end,handler) return a+b+c"[..],
        &b"local a,b,c=1,2,3 xpcall(function() swallow(function() coroutine.resume(coroutine.create(host_panic)) end) end,handler) return a+b+c"[..],
        &b"local a,b,c=1,2,3 xpcall(swallow,handler,function() host_panic() end) return a+b+c"[..],
        &b"local a,b,c=1,2,3 xpcall(swallow,handler,function() coroutine.resume(coroutine.create(host_panic)) end) return a+b+c"[..],
    ] {
        let mut lua = interpreter();
        register(&mut lua, "host_panic", Rc::new(|_, _| panic!("nested host panic")));
        register(&mut lua, "swallow", Rc::new(|vm, args| {
            let function = vm.check_any(args, 0)?;
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.call(function, &[])));
            assert!(caught.is_err());
            Ok(0)
        }));
        let handled = Rc::new(Cell::new(false));
        let observed = handled.clone();
        register(&mut lua, "handler", Rc::new(move |vm, args| {
            observed.set(true);
            let value = vm.arg(args, 0);
            vm.results(&[value])
        }));
        let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lua.exec("nested_panic", source)));
        assert!(matches!(ended, Ok(Err(Error::Runtime { message, .. })) if message.contains("interpreter retired")));
        assert!(!handled.get(), "retirement bypasses Lua error handlers");
        assert!(lua.exec("after", b"return 71").is_err());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, test)]
fn a_payload_drop_panic_retires_explicit_collection() {
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("payload drop panic");
        }
    }
    let mut lua = interpreter();
    lua.userdata(Box::new(PanicOnDrop), None)
        .expect("unrooted payload");
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lua.collect()));
    assert!(ended.is_err());
    assert!(lua.collect().is_err());
    assert!(lua.exec("after", b"return 71").is_err());
}


#[cfg(not(target_arch = "wasm32"))]
fn install_panicky(lua: &mut Lua, effects: Rc<Cell<u32>>) {
    register(lua, "host_panic", Rc::new(|_, _| panic!("probe panic")));
    register(
        lua,
        "swallow",
        Rc::new(|vm, args| {
            let function = vm.check_any(args, 0)?;
            let caught = catch_unwind(AssertUnwindSafe(|| vm.call(function, &[])));
            assert!(caught.is_err());
            Ok(0)
        }),
    );
    register(
        lua,
        "side_effect",
        Rc::new(move |_, _| {
            effects.set(effects.get() + 1);
            Ok(0)
        }),
    );
}


#[cfg(not(target_arch = "wasm32"))]
fn poke_retired(lua: &mut Lua) {
    let globals = lua.globals();
    let poked = catch_unwind(AssertUnwindSafe(|| {
        let _ = lua.string(b"x");
        let _ = lua.global("x");
        let _ = lua.set_global("x", Value::Nil);
        let _ = lua.load("x", b"return 1");
        let _ = lua.collect();
        let _ = lua.pin(Value::Table(globals));
        let _ = lua.unpin(Value::Table(globals));
        let _ = lua.userdata(Box::new(1u8), None);
        let _ = lua.function("f", Rc::new(|_, _| Ok(0)));
        lua.begin_tick();
        let _ = lua.exec("x", b"return 1");
        let vm = lua.vm();
        let _ = vm.new_table();
        let _ = vm.raw_get(globals, Value::Number(1.0));
        let _ = vm.raw_set(globals, Value::Number(1.0), Value::Nil);
        let _ = vm.raw_set(globals, Value::Nil, Value::Nil);
        let _ = vm.set_metatable(globals, None);
        let _ = vm.new_string(b"x");
        let _ = vm.error("x");
        let _ = vm.pin(Value::Table(globals));
        let _ = vm.unpin(Value::Table(globals));
        let _ = vm.userdata(1u8, None, 0);
        let _ = vm.results(&[Value::Nil]);
        let _ = vm.call(Value::Nil, &[]);
        let _ = vm.charge_fuel(1);
        let _ = lua.memory();
        let _ = lua.tick_fuel_left();
    }));
    assert!(poked.is_ok(), "a retired interpreter panicked");
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, test)]
fn payload_drop_panic_during_script_gc_retires() {
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("payload drop");
        }
    }
    let mut lua = interpreter();
    lua.userdata(Box::new(PanicOnDrop), None).expect("payload");
    lua.set_gc_stress(GcStress::Full);
    let ended = catch_unwind(AssertUnwindSafe(|| {
        lua.exec("gc", b"local t = {} for i = 1, 100 do t[i] = {} end")
    }));
    assert!(ended.is_err(), "{ended:?}");
    assert!(lua.exec("after", b"return 1").is_err());
    poke_retired(&mut lua);
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(test, test)]
fn payload_drop_panic_inside_nested_vm_call_caught_by_the_callback_retires() {
    struct PanicOnDrop;
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("payload drop");
        }
    }
    let effects = Rc::new(Cell::new(0));
    let mut lua = interpreter();
    install_panicky(&mut lua, effects.clone());
    register(
        &mut lua,
        "bomb",
        Rc::new(|vm, _| {
            vm.userdata(PanicOnDrop, None, 0)?;
            Ok(0)
        }),
    );
    lua.set_gc_stress(GcStress::Full);
    let ended = catch_unwind(AssertUnwindSafe(|| {
        lua.exec(
            "gc",
            b"swallow(function() bomb() local t = {} for i = 1, 100 do t[i] = {} end end) side_effect() return 1",
        )
    }));
    assert!(
        matches!(&ended, Ok(Err(Error::Runtime { message, .. })) if message.contains("retired")),
        "{ended:?}"
    );
    assert_eq!(effects.get(), 0);
    poke_retired(&mut lua);
}
