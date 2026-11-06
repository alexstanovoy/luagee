//! External embedding contracts, also run by the single-threaded wasm runner.

use std::rc::Rc;

use luagee::{Error, GcStress, Keys, Limit, Limits, Lua};

#[path = "support/host_api.rs"]
mod callbacks;

fn interpreter(limits: Limits) -> Lua {
    let mut lua = Lua::new(
        limits,
        Keys {
            hash: [1, 2],
            random: 3,
        },
    )
    .expect("interpreter");
    callbacks::install(&mut lua).expect("host callbacks");
    lua
}

#[cfg(not(test))]
pub fn contracts() {
    callbacks_preserve_values_and_protect_userdata();
    callbacks_keep_roots_across_nested_collection();
    nested_calls_keep_limits_and_the_interpreter_recovers();
    host_allocations_and_roots_are_bounded();
    invalid_result_counts_are_errors();
    userdata_and_root_storage_stay_charged_after_collection();
    nested_runtime_errors_can_be_handled_inside_a_callback();
    metatable_writes_and_duplicate_roots_survive_collection();
    deepest_host_reentry_keeps_stack_margin();
    stale_handles_are_rejected_after_collection_and_slot_reuse();
    script_owned_tables_support_raw_operations_with_script_keys();
    typed_callback_payloads_and_invalid_result_counts_are_checked();
    callback_hash_churn_and_migration_preserve_table_state();
    callback_fuzzer_fixture_retries_modules_on_the_next_tick();
    foreign_interpreter_probe_rejects_payloads_and_argument_contexts();
}

#[cfg_attr(test, test)]
fn callbacks_preserve_values_and_protect_userdata() {
    let mut lua = interpreter(Limits::DEFAULT);
    lua.exec(
        "embedding",
        br#"
        local n, empty, missing = host.arity()
        assert(n == 0 and empty and missing == nil)
        n, empty, missing = host.arity(nil, false)
        assert(n == 2 and not empty and missing == nil)
        assert(host.string('hello') == 'hello')
        assert(host.invoke(function(a, b) return a + b end, 3, 4) == 7)
        local t = host.make(42)
        assert(host.read(t) == 42)
        assert(host.convert('8.9', nil, 123, {}) == 8)
        assert(host.optional() == 7)
        local u = host.object(9)
        assert(host.object_value(u) == 9)
        assert(getmetatable(u) == 'sealed')
        assert(not pcall(setmetatable, u, {}))
        assert(not pcall(host.object_value, setmetatable({}, {})))
        assert(not pcall(host.object_value, {}))
        assert(not pcall(host.read, 1))
        assert(not pcall(host.convert, {}))
        assert(not pcall(host.bad_key, 0/0))
        assert(not pcall(host.bad_key, nil))
        local a, b, c = host.invoke(function() return 1, nil, 3 end)
        assert(a == 1 and b == nil and c == 3)
        local co = coroutine.create(function()
            local ok, message = pcall(host.invoke, coroutine.yield)
            assert(not ok and message:find('C-call boundary', 1, true))
            coroutine.yield(5)
            return host.optional()
        end)
        local ok, v = coroutine.resume(co)
        assert(ok and v == 5)
        ok, v = coroutine.resume(co)
        assert(ok and v == 7)
        local ok, message = pcall(host.invoke, function() error('nested failure') end)
        assert(not ok and message:find('nested failure', 1, true))
        assert(host.invoke(function() return 19 end) == 19)
        local ok, message = pcall(host.convert, false)
        assert(not ok and message:find("number expected, got boolean", 1, true))
    "#,
    )
    .expect("callbacks run");
}

#[cfg_attr(test, test)]
fn callbacks_keep_roots_across_nested_collection() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter(Limits {
            fuel_per_call: 5_000_000,
            fuel_per_tick: 20_000_000,
            ..Limits::DEFAULT
        });
        lua.set_gc_stress(stress);
        lua.exec(
            "roots",
            br#"
            for i = 1, 100 do
                local t = host.collect(function() collectgarbage() end, i)
                assert(host.read(t) == i)
            end
            host.remember(host.make(71))
            collectgarbage()
            assert(host.read(host.recall()) == 71)
            host.forget()
            collectgarbage()
            assert(host.recall() == nil)
        "#,
        )
        .expect("rooted handles survive");
    }
}

#[cfg_attr(test, test)]
fn nested_calls_keep_limits_and_the_interpreter_recovers() {
    for (fuel_per_call, fuel_per_tick, expected) in [
        (20_000, 40_000, Limit::FuelPerCall),
        (40_000, 20_000, Limit::FuelPerTick),
    ] {
        let mut lua = interpreter(Limits {
            fuel_per_call,
            fuel_per_tick,
            ..Limits::DEFAULT
        });
        let ended = lua.exec(
            "fuel",
            b"pcall(host.invoke, function() while true do host.optional() end end)",
        );
        assert!(
            matches!(ended, Err(Error::Limit(limit)) if limit == expected),
            "{ended:?}"
        );
        lua.begin_tick();
        lua.exec(
            "after",
            b"assert(host.invoke(function() return 12 end) == 12)",
        )
        .expect("recovers");
        lua.begin_tick();
        let ended = lua.exec("callback_fuel", b"pcall(host.burn)");
        assert!(
            matches!(ended, Err(Error::Limit(limit)) if limit == expected),
            "{ended:?}"
        );
        lua.begin_tick();
        let ended = lua.exec(
            "recursion",
            b"local function f() return host.invoke(f) end f()",
        );
        assert!(
            matches!(ended, Err(Error::Runtime { message, .. }) if message.contains("C stack overflow"))
        );
        lua.begin_tick();
        lua.exec("after_recursion", b"assert(host.optional() == 7)")
            .expect("recovers");
    }
}

#[cfg_attr(test, test)]
fn host_allocations_and_roots_are_bounded() {
    for source in [
        &b"local t = {} while true do t[#t+1] = host.object(1) end"[..],
        &b"local t = host.make(1) while true do host.remember(t) end"[..],
        &b"while true do host.retain() end"[..],
    ] {
        let mut lua = interpreter(Limits {
            memory: 256 << 10,
            fuel_per_call: 10_000_000,
            fuel_per_tick: 10_000_000,
            ..Limits::DEFAULT
        });
        let ended = lua.exec("memory", source);
        assert!(
            matches!(ended, Err(Error::Limit(Limit::Memory))),
            "{ended:?}"
        );
    }
}

#[cfg_attr(test, test)]
fn invalid_result_counts_are_errors() {
    let mut lua = interpreter(Limits::DEFAULT);
    let bad = lua
        .function("bad", Rc::new(|_, _| Ok(usize::MAX)))
        .expect("callback");
    lua.set_global("bad", bad).expect("global");
    let ended = lua.exec("bad_count", b"bad()");
    assert!(
        matches!(ended, Err(Error::Runtime { message, .. }) if message.contains("invalid host result count"))
    );
    lua.exec("after_bad_count", b"return 1").expect("recovers");
}

#[cfg_attr(test, test)]
fn userdata_and_root_storage_stay_charged_after_collection() {
    let mut lua = interpreter(Limits::DEFAULT);
    lua.collect().expect("baseline collection");
    let before = lua.memory();
    let value = lua.vm().userdata([1u8; 4096], None, 8192).expect("object");
    lua.pin(value).expect("root");
    lua.collect().expect("collection");
    assert!(lua.memory() >= before + 12288);
    lua.unpin(value).expect("release");
    lua.collect().expect("collection");
    assert!(lua.memory() < before + 12288);
    let ended = lua.vm().userdata(0u64, None, usize::MAX);
    assert!(matches!(ended, Err(luagee::LuaError::Limit(Limit::Memory))));
}

#[cfg_attr(test, test)]
fn nested_runtime_errors_can_be_handled_inside_a_callback() {
    let mut lua = interpreter(Limits::DEFAULT);
    let value = lua
        .function(
            "recover",
            Rc::new(|vm, args| {
                let first = vm.check_any(args, 0)?;
                match vm.call(first, &[]) {
                    Err(luagee::LuaError::Runtime(thrown)) => {
                        vm.pin(thrown.value)?;
                        let second = vm.check_any(args, 1)?;
                        let result = vm.call(second, &[]);
                        vm.unpin(thrown.value)?;
                        result?;
                        vm.results(&[thrown.value])
                    }
                    Err(limit) => Err(limit),
                    Ok(values) => vm.results(&values),
                }
            }),
        )
        .expect("callback");
    lua.set_global("recover", value).expect("global");
    for stress in [GcStress::Full, GcStress::Incremental] {
        lua.set_gc_stress(stress);
        lua.exec("catch", b"local e = recover(function() error({message = 'kept'}) end, function() collectgarbage() end) assert(e.message == 'kept')").expect("catches and reenters");
    }
}

#[cfg_attr(test, test)]
fn metatable_writes_and_duplicate_roots_survive_collection() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter(Limits::DEFAULT);
        lua.pin(luagee::Value::Number(f64::NAN)).expect("NaN root");
        assert!(
            lua.unpin(luagee::Value::Number(f64::NAN))
                .expect("release NaN")
        );
        assert!(
            !lua.unpin(luagee::Value::Number(f64::NAN))
                .expect("NaN absent")
        );
        let table = lua.vm().new_table().expect("table");
        lua.pin(luagee::Value::Table(table)).expect("root");
        lua.pin(luagee::Value::Table(table)).expect("second root");
        lua.unpin(luagee::Value::Table(table))
            .expect("one root remains");
        let meta = lua.vm().new_table().expect("metatable");
        let index = lua.vm().new_string(b"__index").expect("key");
        let fallback = lua.vm().new_table().expect("fallback");
        let key = lua.vm().new_string(b"value").expect("key");
        lua.vm()
            .raw_set(fallback, key, luagee::Value::Number(19.0))
            .expect("field");
        lua.vm()
            .raw_set(meta, index, luagee::Value::Table(fallback))
            .expect("index");
        lua.vm()
            .set_metatable(table, Some(meta))
            .expect("metatable");
        lua.set_gc_stress(stress);
        lua.collect().expect("collection");
        lua.set_global("object", luagee::Value::Table(table))
            .expect("global");
        lua.exec("meta", b"assert(object.value == 19)")
            .expect("metatable survives");
        lua.unpin(luagee::Value::Table(table)).expect("last root");
        assert!(
            !lua.unpin(luagee::Value::Table(table))
                .expect("missing root")
        );
    }
}

#[cfg_attr(test, test)]
fn deepest_host_reentry_keeps_stack_margin() {
    let run = || {
        let mut lua = interpreter(Limits {
            fuel_per_call: 5_000_000,
            fuel_per_tick: 5_000_000,
            ..Limits::DEFAULT
        });
        lua.exec("deep_host", br#"
            local blocks = ('for i=1,1 do '):rep(199) .. ('end '):rep(199)
            local calls = 'return ' .. ('f('):rep(198) .. '1' .. (')'):rep(198)
            local pattern = ('a?'):rep(199) .. 'a'
            local function nest(n)
                if n > 0 then return host.invoke(nest, n - 1) end
                local compiled, message = loadstring(blocks)
                assert(compiled or message:find('function or expression too complex', 1, true))
                assert(loadstring(calls))
                assert(loadstring('return ' .. ('function() return '):rep(66) .. '1' .. (' end'):rep(66)))
                assert(('a'):match(pattern) == 'a')
            end
            nest(98)
        "#).expect("deep host reentry fits");
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(run)
        .expect("thread")
        .join()
        .expect("stack margin");
    #[cfg(target_arch = "wasm32")]
    run();
}

#[cfg_attr(test, test)]
fn stale_handles_are_rejected_after_collection_and_slot_reuse() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter(Limits::DEFAULT);
        let stale_object = lua.vm().userdata(17u64, None, 0).expect("object");
        let stale_string = lua.vm().new_string(b"unrooted bytes").expect("string");
        let stale_table = lua.vm().new_table().expect("table");
        let stale_function = lua.load("old", b"return 1").expect("function");
        lua.set_gc_stress(stress);
        lua.collect().expect("frees unrooted handles");
        lua.begin_tick();
        let replacement = lua.vm().userdata(29u64, None, 0).expect("replacement");
        lua.pin(replacement).expect("root");
        assert_eq!(lua.vm().userdata_ref::<u64>(stale_object), None);
        assert_eq!(lua.userdata_ref::<u64>(stale_object), None);
        assert_eq!(lua.vm().bytes(stale_string), None);
        assert_eq!(lua.bytes(stale_string), None);
        assert!(lua.pin(stale_object).is_err());
        assert!(
            lua.vm()
                .raw_get(stale_table, luagee::Value::Number(1.0))
                .is_err()
        );
        assert!(
            lua.vm()
                .raw_set(stale_table, luagee::Value::Number(1.0), replacement)
                .is_err()
        );
        assert!(lua.vm().set_metatable(stale_table, None).is_err());
        assert!(lua.set_global("stale", stale_object).is_err());
        let live = lua.vm().new_table().expect("live table");
        assert!(lua.vm().raw_get(live, stale_string).is_err());
        assert!(
            lua.vm()
                .raw_set(live, luagee::Value::Number(1.0), stale_object)
                .is_err()
        );
        assert!(lua.vm().set_metatable(live, Some(stale_table)).is_err());
        assert!(lua.call(stale_function, &[]).is_err());
        assert_eq!(lua.vm().userdata_ref::<u64>(replacement), Some(&29));
        lua.begin_tick();
        lua.exec(
            "stale_callback",
            include_bytes!("host_api/stale_handles.lua"),
        )
        .expect("stale callback handles rejected");
    }
}

#[cfg_attr(test, test)]
fn script_owned_tables_support_raw_operations_with_script_keys() {
    for stress in [GcStress::Off, GcStress::Incremental] {
        let mut lua = interpreter(Limits {
            fuel_per_call: 5_000_000,
            fuel_per_tick: 5_000_000,
            ..Limits::DEFAULT
        });
        lua.set_gc_stress(stress);
        lua.exec("raw_tables", include_bytes!("host_api/raw_tables.lua"))
            .expect("raw API accepts script-owned tables and validates keys");
    }
}

#[cfg_attr(test, test)]
fn typed_callback_payloads_and_invalid_result_counts_are_checked() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter(Limits::DEFAULT);
        lua.set_gc_stress(stress);
        lua.exec(
            "typed_objects",
            include_bytes!("host_api/typed_objects.lua"),
        )
        .expect("payload types and result counts checked");
    }
}

#[cfg_attr(test, test)]
fn callback_hash_churn_and_migration_preserve_table_state() {
    let mut lua = interpreter(Limits {
        fuel_per_call: 5_000_000,
        fuel_per_tick: 5_000_000,
        ..Limits::DEFAULT
    });
    lua.exec("hash_churn", include_bytes!("host_api/hash_churn.lua"))
        .expect("budgeted compaction and migration");
}

#[cfg_attr(test, test)]
fn callback_fuzzer_fixture_retries_modules_on_the_next_tick() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter(Limits::DEFAULT);
        lua.set_gc_stress(stress);
        let ended = lua.exec("first_attempt", include_bytes!("host_api/retry_module.lua"));
        assert!(
            matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
            "{ended:?}"
        );
        lua.begin_tick();
        lua.exec("retry_attempt", include_bytes!("host_api/retry_module.lua"))
            .expect("module retries after the limit");
    }
}

#[cfg_attr(test, test)]
fn foreign_interpreter_probe_rejects_payloads_and_argument_contexts() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter(Limits::DEFAULT);
        lua.set_gc_stress(stress);
        lua.exec(
            "foreign_handles",
            include_bytes!("host_api/foreign_handles.lua"),
        )
        .expect("foreign data never aliases this interpreter");
    }
}
