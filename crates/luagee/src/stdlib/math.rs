//! The math library: every function through `number::math`, the numeric rule's one table; `random` from the
//! host-seeded stream.

use crate::number::{self, math};
use crate::value::Value;
use crate::vm::{Args, LuaError, Vm};

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    let table = vm.library(
        "math",
        &[
            ("abs", |vm, a| unary(vm, a, math::abs)),
            ("acos", |vm, a| unary(vm, a, math::acos)),
            ("asin", |vm, a| unary(vm, a, math::asin)),
            ("atan", |vm, a| unary(vm, a, math::atan)),
            ("atan2", atan2),
            ("ceil", |vm, a| unary(vm, a, math::ceil)),
            ("cos", |vm, a| unary(vm, a, math::cos)),
            ("cosh", |vm, a| unary(vm, a, math::cosh)),
            ("deg", |vm, a| unary(vm, a, math::deg)),
            ("exp", |vm, a| unary(vm, a, math::exp)),
            ("floor", |vm, a| unary(vm, a, math::floor)),
            ("fmod", fmod),
            ("frexp", frexp),
            ("ldexp", ldexp),
            ("log", log),
            ("log10", |vm, a| unary(vm, a, math::log10)),
            ("max", max),
            ("min", min),
            ("modf", modf),
            ("pow", pow),
            ("rad", |vm, a| unary(vm, a, math::rad)),
            ("random", random),
            ("randomseed", randomseed),
            ("sin", |vm, a| unary(vm, a, math::sin)),
            ("sinh", |vm, a| unary(vm, a, math::sinh)),
            ("sqrt", |vm, a| unary(vm, a, math::sqrt)),
            ("tan", |vm, a| unary(vm, a, math::tan)),
            ("tanh", |vm, a| unary(vm, a, math::tanh)),
        ],
    )?;
    vm.set_field(table, "pi", Value::Number(std::f64::consts::PI))?;
    vm.set_field(table, "huge", Value::Number(f64::INFINITY))?;
    Ok(())
}

fn unary(vm: &mut Vm, args: Args, f: fn(f64) -> f64) -> Result<usize, LuaError> {
    let x = vm.check_number(args, 0)?;
    vm.results(&[Value::Number(f(x))])
}

fn atan2(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let y = vm.check_number(args, 0)?;
    let x = vm.check_number(args, 1)?;
    vm.results(&[Value::Number(math::atan2(y, x))])
}

fn fmod(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let a = vm.check_number(args, 0)?;
    let b = vm.check_number(args, 1)?;
    vm.results(&[Value::Number(math::fmod(a, b))])
}

fn pow(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let a = vm.check_number(args, 0)?;
    let b = vm.check_number(args, 1)?;
    vm.results(&[Value::Number(number::pow(a, b))])
}

fn log(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = vm.check_number(args, 0)?;
    let value = match vm.arg(args, 1) {
        Value::Nil => math::ln(x),
        _ => {
            let base = vm.check_number(args, 1)?;
            number::div(math::ln(x), math::ln(base))
        }
    };
    vm.results(&[Value::Number(value)])
}

fn modf(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = vm.check_number(args, 0)?;
    let (integer, fraction) = math::modf(x);
    vm.results(&[Value::Number(integer), Value::Number(fraction)])
}

fn frexp(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let x = vm.check_number(args, 0)?;
    let (mantissa, exponent) = math::frexp(x);
    vm.results(&[Value::Number(mantissa), Value::Number(f64::from(exponent))])
}

fn ldexp(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let m = vm.check_number(args, 0)?;
    let e = vm.check_integer(args, 1)?;
    let e = i32::try_from(e.clamp(-100_000, 100_000)).expect("clamped into i32");
    vm.results(&[Value::Number(math::ldexp(m, e))])
}

fn max(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let mut best = vm.check_number(args, 0)?;
    for i in 1..args.count {
        let x = vm.check_number(args, i)?;
        if x > best {
            best = x;
        }
    }
    vm.results(&[Value::Number(best)])
}

fn min(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let mut best = vm.check_number(args, 0)?;
    for i in 1..args.count {
        let x = vm.check_number(args, i)?;
        if x < best {
            best = x;
        }
    }
    vm.results(&[Value::Number(best)])
}

fn random(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let unit = math::unit(vm.random.next());
    let value = match args.count {
        0 => unit,
        1 | 2 => {
            let (low, high) = if args.count == 1 {
                (1.0, vm.check_number(args, 0)?)
            } else {
                (vm.check_number(args, 0)?, vm.check_number(args, 1)?)
            };
            let (low, high) = (math::floor(low), math::floor(high));
            if low > high {
                return Err(vm.arg_error(args, args.count - 1, "interval is empty"));
            }
            // floor(unit * (high - low + 1)) + low
            let span = number::add(number::sub(high, low), 1.0);
            number::add(math::floor(number::mul(unit, span)), low)
        }
        _ => return Err(vm.error("wrong number of arguments")),
    };
    vm.results(&[Value::Number(value)])
}

fn randomseed(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let seed = vm.check_number(args, 0)?;
    vm.random.seed(seed.to_bits());
    Ok(0)
}
