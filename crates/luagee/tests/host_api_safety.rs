//! Security regressions at the public embedding boundary.

use std::cell::Cell;
use std::rc::Rc;

use luagee::{Args, Error, GcStress, Keys, Limits, Lua, Value};

fn interpreter() -> Lua {
    Lua::new(Limits { fuel_per_call: 20_000, fuel_per_tick: 40_000, ..Limits::DEFAULT }, Keys { hash: [17, 19], random: 23 }).expect("interpreter")
}

fn register(lua: &mut Lua, name: &'static str, callback: luagee::HostFunction) {
    let value = lua.function(name, callback).expect("callback");
    lua.set_global(name, value).expect("global");
}

#[cfg(not(test))]
pub fn contracts() {
    top_level_vm_calls_cannot_bypass_tick_accounting();
    host_table_writes_have_linear_cost_for_a_growing_list();
    saved_argument_contexts_cannot_read_a_later_callback();
    nested_results_remain_live_across_reentry_without_explicit_pins();
}

#[cfg_attr(test, test)]
fn top_level_vm_calls_cannot_bypass_tick_accounting() {
    let mut lua = interpreter();
    let function = lua.load("spin", b"for i=1,100 do end").expect("function");
    lua.pin(function).expect("pin");
    lua.begin_tick();
    let before = lua.tick_fuel_left();
    let ended = lua.vm().call(function, &[]);
    assert!(matches!(ended, Err(luagee::LuaError::Runtime(_))), "{ended:?}");
    assert_eq!(lua.tick_fuel_left(), before);
    lua.call(function, &[]).expect("top level uses Lua::call");
    assert!(lua.tick_fuel_left() < before);
}

#[cfg_attr(test, test)]
fn saved_argument_contexts_cannot_read_a_later_callback() {
    let mut lua = interpreter();
    let saved = Rc::new(Cell::<Option<Args>>::new(None));
    let capture = saved.clone();
    register(&mut lua, "save", Rc::new(move |_, args| { capture.set(Some(args)); Ok(0) }));
    register(&mut lua, "read", Rc::new(move |vm, _| {
        let args = saved.get().expect("the script saved an argument context");
        assert_eq!(vm.arg(args, 0), Value::Nil);
        assert!(vm.opt_integer(args, 0, 0).is_err());
        assert!(vm.opt_string(args, 0).is_err());
        assert!(vm.check_table(args, 0).is_err());
        let value = vm.check_any(args, 0)?;
        vm.results(&[value])
    }));
    let ended = lua.exec("expired", b"save(17) return read(29)");
    assert!(matches!(ended, Err(Error::Runtime { message, .. }) if message.contains("expired callback arguments")));
    lua.exec("after_expired", b"return 7").expect("interpreter recovers");
    register(&mut lua, "outer", Rc::new(|vm, args| {
        let function = vm.check_any(args, 0)?;
        vm.call(function, &[])?;
        let value = vm.check_any(args, 1)?;
        vm.results(&[value])
    }));
    lua.exec("coroutine_scope", br#"
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
    "#).expect("coroutines restore only their enclosing argument context");
}

#[cfg_attr(test, test)]
fn nested_results_remain_live_across_reentry_without_explicit_pins() {
    for stress in [GcStress::Full, GcStress::Incremental] {
        let mut lua = interpreter();
        register(&mut lua, "retain_results", Rc::new(|vm, args| {
            let values = vm.call(vm.arg(args, 0), &[])?;
            vm.call(vm.arg(args, 1), &[])?;
            vm.results(&values)
        }));
        lua.set_gc_stress(stress);
        lua.exec("nested_results", b"local t = retain_results(function() return {value = 71} end, function() collectgarbage() end) assert(t.value == 71)").expect("nested results are callback-local roots");
    }
}

#[cfg_attr(test, test)]
fn host_table_writes_have_linear_cost_for_a_growing_list() {
    let mut lua = interpreter();
    register(&mut lua, "fill", Rc::new(|vm, _| {
        let table = vm.new_table()?;
        for index in 1..=1000 {
            vm.raw_set(table, Value::Number(f64::from(index)), Value::Number(f64::from(index)))?;
        }
        for index in 1..=1000 {
            vm.raw_set(table, Value::Number(1.0), Value::Number(f64::from(index)))?;
        }
        vm.results(&[Value::Table(table)])
    }));
    lua.exec("list", b"local t = fill() assert(#t == 1000 and t[1000] == 1000 and t[1] == 1000)").expect("linear writes fit the call budget");
}
