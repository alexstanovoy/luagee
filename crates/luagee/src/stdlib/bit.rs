//! LuaJIT's `bit` library: 32-bit operations on the doubles' integer values.

use crate::number::to_bit;
use crate::value::Value;
use crate::vm::{Args, LuaError, Vm};

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    vm.library(
        "bit",
        &[
            ("tobit", tobit),
            ("tohex", tohex),
            ("bnot", bnot),
            ("band", |vm, a| fold(vm, a, |x, y| x & y)),
            ("bor", |vm, a| fold(vm, a, |x, y| x | y)),
            ("bxor", |vm, a| fold(vm, a, |x, y| x ^ y)),
            ("lshift", |vm, a| {
                shift(vm, a, |x, n| ((x as u32) << n) as i32)
            }),
            ("rshift", |vm, a| {
                shift(vm, a, |x, n| ((x as u32) >> n) as i32)
            }),
            ("arshift", |vm, a| shift(vm, a, |x, n| x >> n)),
            ("rol", |vm, a| {
                shift(vm, a, |x, n| (x as u32).rotate_left(n) as i32)
            }),
            ("ror", |vm, a| {
                shift(vm, a, |x, n| (x as u32).rotate_right(n) as i32)
            }),
            ("bswap", bswap),
        ],
    )?;
    Ok(())
}

fn bits(vm: &mut Vm, args: Args, i: usize) -> Result<i32, LuaError> {
    Ok(to_bit(vm.check_number(args, i)?))
}

fn result(vm: &mut Vm, x: i32) -> Result<usize, LuaError> {
    vm.results(&[Value::Number(f64::from(x))])
}

fn tobit(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = bits(vm, args, 0)?;
    result(vm, x)
}

fn bnot(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = bits(vm, args, 0)?;
    result(vm, !x)
}

fn bswap(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = bits(vm, args, 0)?;
    result(vm, x.swap_bytes())
}

fn fold(vm: &mut Vm, args: Args, op: fn(i32, i32) -> i32) -> Result<usize, LuaError> {
    let mut x = bits(vm, args, 0)?;
    for i in 1..args.count {
        let y = bits(vm, args, i)?;
        x = op(x, y);
    }
    result(vm, x)
}

fn shift(vm: &mut Vm, args: Args, op: fn(i32, u32) -> i32) -> Result<usize, LuaError> {
    let x = bits(vm, args, 0)?;
    let n = (bits(vm, args, 1)? & 31) as u32;
    result(vm, op(x, n))
}

fn tohex(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = bits(vm, args, 0)? as u32;
    let n = match vm.arg(args, 1) {
        Value::Nil => 8,
        _ => bits(vm, args, 1)?,
    };
    let (upper, digits) = if n < 0 {
        (true, n.unsigned_abs())
    } else {
        (false, n as u32)
    };
    // LuaJIT gives up to 254 digits, the value zero-extended past its 8.
    let digits = digits.min(254) as usize;
    let text = if upper {
        format!("{x:0254X}")
    } else {
        format!("{x:0254x}")
    };
    vm.push_string(&text.as_bytes()[254 - digits..])
}
