//! The table library, with LuaJIT's 5.1 extras (`foreach`, `foreachi`, `getn`, `maxn`); every element moved or
//! compared is paid in fuel.

use crate::value::{TableId, Value};
use crate::vm::{Args, LuaError, Vm};

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    vm.library(
        "table",
        &[
            ("concat", concat),
            ("foreach", foreach),
            ("foreachi", foreachi),
            ("getn", getn),
            ("insert", insert),
            ("maxn", maxn),
            ("remove", remove),
            ("sort", sort),
        ],
    )?;
    Ok(())
}

fn get(vm: &Vm, t: TableId, i: i64) -> Value {
    vm.heap.tables.get(t.0).get_int(i, &vm.heap.hasher)
}

fn set(vm: &mut Vm, t: TableId, i: i64, value: Value) -> Result<(), LuaError> {
    vm.heap.raw_set(t, Value::Number(i as f64), value)
}

fn length(vm: &Vm, t: TableId) -> i64 {
    vm.heap.table(t).border() as i64
}

fn getn(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let n = length(vm, t);
    vm.results(&[Value::Number(n as f64)])
}

fn maxn(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let table = vm.heap.table(t);
    vm.charge_fuel(table.pairs().count() as u64)?;
    let most = vm.heap.table(t).max_integer_key();
    vm.results(&[Value::Number(most)])
}

fn insert(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let n = length(vm, t);
    match args.count {
        2 => {
            let value = vm.arg(args, 1);
            set(vm, t, n + 1, value)?;
        }
        3 => {
            let position = vm.check_integer(args, 1)?;
            let value = vm.arg(args, 2);
            if position <= n {
                vm.charge_fuel((n - position.max(0)) as u64)?;
                for i in (position..=n).rev() {
                    let moved = get(vm, t, i);
                    set(vm, t, i + 1, moved)?;
                }
            }
            set(vm, t, position, value)?;
        }
        _ => return Err(vm.error("wrong number of arguments to 'insert'")),
    }
    Ok(0)
}

fn remove(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let n = length(vm, t);
    let position = vm.opt_integer(args, 1, n)?;
    // LuaJIT removes nothing (and returns nothing) outside 1..n.
    if position < 1 || position > n {
        return Ok(0);
    }
    let removed = get(vm, t, position);
    vm.charge_fuel((n - position).max(0) as u64)?;
    for i in position..n {
        let moved = get(vm, t, i + 1);
        set(vm, t, i, moved)?;
    }
    if position <= n {
        set(vm, t, n, Value::Nil)?;
    }
    vm.results(&[removed])
}

fn concat(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let separator = match vm.opt_string(args, 1)? {
        Some(s) => vm.heap.string(s).to_vec(),
        None => Vec::new(),
    };
    let first = vm.opt_integer(args, 2, 1)?;
    let last = match vm.arg(args, 3) {
        Value::Nil => length(vm, t),
        _ => vm.check_integer(args, 3)?,
    };
    let mut out = Vec::new();
    let mut i = first;
    while i <= last {
        match get(vm, t, i) {
            Value::String(s) => out.extend_from_slice(vm.heap.string(s)),
            Value::Number(n) => out.extend_from_slice(crate::number::format(n).as_bytes()),
            other => {
                let message = format!(
                    "invalid value ({}) at index {i} in table for 'concat'",
                    other.type_name()
                );
                return Err(vm.error(message));
            }
        }
        if i < last {
            out.extend_from_slice(&separator);
        }
        if out.len() > vm.limits.string_length {
            return Err(vm.string_overflow());
        }
        vm.charge_fuel(1)?;
        i += 1;
    }
    vm.charge_fuel(out.len() as u64 / 16)?;
    vm.push_string(&out)
}

fn foreach(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let f = vm.arg(args, 1);
    let mut key = Value::Nil;
    loop {
        let hash = vm.heap.key_hash(key);
        let Ok((found, skipped)) = vm.heap.table(t).next(key, hash) else {
            return Ok(0);
        };
        vm.charge_fuel(skipped as u64 + 1)?;
        let Some((k, v)) = found else { return Ok(0) };
        let result = vm.call_meta(f, &[k, v])?;
        if result != Value::Nil {
            return vm.results(&[result]);
        }
        key = k;
    }
}

fn foreachi(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let f = vm.arg(args, 1);
    let n = length(vm, t);
    for i in 1..=n {
        let value = get(vm, t, i);
        let result = vm.call_meta(f, &[Value::Number(i as f64), value])?;
        if result != Value::Nil {
            return vm.results(&[result]);
        }
    }
    Ok(0)
}

// --- sort: Lua 5.1's quicksort, comparisons through the comparator or `<`. -----------------------------------

fn less(vm: &mut Vm, comparator: Value, a: Value, b: Value) -> Result<bool, LuaError> {
    vm.charge_fuel(1)?;
    if comparator == Value::Nil {
        vm.less_than(a, b)
    } else {
        Ok(vm.call_meta(comparator, &[a, b])?.is_truthy())
    }
}

fn sort(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let t = vm.check_table(args, 0)?;
    let comparator = vm.arg(args, 1);
    if !matches!(comparator, Value::Nil | Value::Function(_)) {
        return Err(vm.arg_type_error(args, 1, "function"));
    }
    let n = length(vm, t);
    // The pivot is kept on the stack while comparators run, so the collector sees it.
    let pivot_slot = vm.stack.len();
    vm.push(Value::Nil)?;
    let mut ranges = vec![(1i64, n)];
    while let Some((mut low, mut high)) = ranges.pop() {
        while low < high {
            let (a, b) = (get(vm, t, low), get(vm, t, high));
            if less(vm, comparator, b, a)? {
                swap(vm, t, low, high)?;
            }
            if high - low == 1 {
                break;
            }
            let middle = (low + high) / 2;
            let (a, b) = (get(vm, t, middle), get(vm, t, low));
            if less(vm, comparator, a, b)? {
                swap(vm, t, middle, low)?;
            } else {
                let (a, b) = (get(vm, t, high), get(vm, t, middle));
                if less(vm, comparator, a, b)? {
                    swap(vm, t, middle, high)?;
                }
            }
            if high - low == 2 {
                break;
            }
            let pivot = get(vm, t, middle);
            vm.stack[pivot_slot] = pivot;
            swap(vm, t, middle, high - 1)?;
            let (mut i, mut j) = (low, high - 1);
            loop {
                i += 1;
                while less(vm, comparator, get(vm, t, i), pivot)? {
                    if i >= high {
                        return Err(vm.error("invalid order function for sorting"));
                    }
                    i += 1;
                }
                j -= 1;
                while less(vm, comparator, pivot, get(vm, t, j))? {
                    if j <= low {
                        return Err(vm.error("invalid order function for sorting"));
                    }
                    j -= 1;
                }
                if j < i {
                    break;
                }
                swap(vm, t, i, j)?;
            }
            swap(vm, t, high - 1, i)?;
            // Sort the smaller part next, the larger one later, so the pending ranges stay few.
            if i - low < high - i {
                ranges.push((i + 1, high));
                high = i - 1;
            } else {
                ranges.push((low, i - 1));
                low = i + 1;
            }
        }
    }
    vm.stack.truncate(pivot_slot);
    Ok(0)
}

fn swap(vm: &mut Vm, t: TableId, i: i64, j: i64) -> Result<(), LuaError> {
    let (a, b) = (get(vm, t, i), get(vm, t, j));
    set(vm, t, i, b)?;
    set(vm, t, j, a)
}
