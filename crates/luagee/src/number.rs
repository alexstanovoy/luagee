//! Lua's numbers: IEEE-754 doubles, and the one module of the crate where floating-point arithmetic is allowed.
//!
//! A Lua number is an `f64`; the crate denies `clippy::float_arithmetic` everywhere else. The numeric rule keeps the
//! results reproducible: floating-point arithmetic happens in this module only;
//! each operation is one IEEE-754 binary64 operation rounded to nearest, ties to even (Rust never fuses a multiply and
//! an add, and nothing here calls `mul_add`); numbers are printed and read in Rust, never through C's `printf` or
//! `strtod`; and every NaN prints the same, so no NaN's sign or payload can be observed.
//!
//! The functions beyond the basic operations (`math.sin`, `math.exp`, `math.pow`, ...) will live here too, behind one
//! table: the standard library's today, the pure-Rust `libm` crate once runs must match across platforms.
//!
//! Reading numbers ([`parse`]: `tonumber`, string coercion and the lexer) follows LuaJIT's scanner.
#![allow(
    clippy::float_arithmetic,
    reason = "the numeric rule's one exception: Lua's numbers are doubles"
)]

/// The text of a number, as `tostring` and `print` give it: C's `%.14g`, which Lua 5.1 and LuaJIT both use.
///
/// Fourteen significant digits, trailing zeros dropped; the exponent form below 1e-4 and from 1e14 on. Infinities
/// print as `inf` and `-inf`, and every NaN as `nan`, whatever its sign and payload (LuaJIT's choice; C libraries
/// differ).
#[must_use]
pub fn format(number: f64) -> String {
    if number.is_nan() {
        return "nan".to_owned();
    }
    if number.is_infinite() {
        return if number > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    // Integers below 10^14 print as themselves: the common case, without float formatting.
    if number.trunc() == number
        && number.abs() < 1e14
        && !(number == 0.0 && number.is_sign_negative())
    {
        return (number as i64).to_string();
    }
    // The decimal exponent after rounding to 14 digits decides the form, as it does in C.
    let scientific = format!("{number:.13e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust's exponent form has an 'e'");
    let exponent: i32 = exponent.parse().expect("Rust's exponent is an integer");
    if (-4..14).contains(&exponent) {
        let decimals = usize::try_from(13 - exponent).expect("the exponent is below 14");
        without_trailing_zeros(format!("{number:.decimals$}"))
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            without_trailing_zeros(mantissa.to_owned()),
            exponent.unsigned_abs()
        )
    }
}

fn without_trailing_zeros(mut digits: String) -> String {
    if digits.contains('.') {
        let kept = digits.trim_end_matches('0').trim_end_matches('.').len();
        digits.truncate(kept);
    }
    digits
}

/// Lua 5.1's `%`: `a - floor(a / b) * b`, so the result takes the divisor's sign. A zero or infinite divisor, or an
/// infinite or NaN dividend, gives NaN.
#[must_use]
pub fn modulo(dividend: f64, divisor: f64) -> f64 {
    dividend - (dividend / divisor).floor() * divisor
}

/// 2^63: the doubles from -2^63 up to here are the ones that convert to `i64`.
const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;
/// 2^64.
const TWO_POW_64: f64 = 18_446_744_073_709_551_616.0;

/// `a + b`.
#[must_use]
#[inline]
pub fn add(a: f64, b: f64) -> f64 {
    a + b
}

/// `a - b`.
#[must_use]
#[inline]
pub fn sub(a: f64, b: f64) -> f64 {
    a - b
}

/// `a * b`.
#[must_use]
#[inline]
pub fn mul(a: f64, b: f64) -> f64 {
    a * b
}

/// `a / b`.
#[must_use]
#[inline]
pub fn div(a: f64, b: f64) -> f64 {
    a / b
}

/// `-a`.
#[must_use]
#[inline]
pub fn neg(a: f64) -> f64 {
    -a
}

/// `a ^ b`.
#[must_use]
#[inline]
pub fn pow(a: f64, b: f64) -> f64 {
    math::pow(a, b)
}

/// The integer a double stands for, when it is one and fits: table keys, string positions, counts.
#[must_use]
#[inline]
pub fn to_integer(number: f64) -> Option<i64> {
    // Every double in this range is exactly an i64; the casts round-trip only for integers.
    if (-TWO_POW_63..TWO_POW_63).contains(&number) {
        let integer = number as i64;
        (integer as f64 == number).then_some(integer)
    } else {
        None
    }
}

/// LuaJIT uses the sign bit for numeric-for direction, so +0 and -0 choose opposite bounds.
#[must_use]
pub fn for_in_range(index: f64, limit: f64, step: f64) -> bool {
    if step.is_sign_positive() {
        index <= limit
    } else {
        limit <= index
    }
}

/// A double truncated toward zero and saturated to `i64`, as C's casts in the libraries' integer arguments do on the
/// usual platforms (`string.sub(s, 2.7)` is `string.sub(s, 2)`); NaN is 0.
#[must_use]
#[inline]
pub fn truncate(number: f64) -> i64 {
    number as i64
}

/// A double as LuaJIT converts it to a 64-bit integer for `%d` (on x86-64): truncated toward zero, and the "integer
/// indefinite" value (the smallest i64) for NaN and anything out of range.
#[must_use]
pub fn to_int64(number: f64) -> i64 {
    if (-TWO_POW_63..TWO_POW_63).contains(&number) {
        number as i64
    } else {
        i64::MIN
    }
}

/// A double as LuaJIT converts it to an unsigned 64-bit integer for `%x`, `%o` and `%u`: values from 2^63 wrap
/// through the signed range.
#[must_use]
pub fn to_uint64(number: f64) -> u64 {
    if number >= TWO_POW_63 {
        to_int64(number - TWO_POW_64) as u64
    } else {
        to_int64(number) as u64
    }
}

/// LuaJIT's `bit.tobit`: the double's value modulo 2^32, as a signed 32-bit integer.
#[must_use]
pub fn to_bit(number: f64) -> i32 {
    // Adding 2^52 + 2^51 puts the integer part's low bits in the mantissa, as LuaJIT does.
    let shifted = number + 6_755_399_441_055_744.0;
    shifted.to_bits() as u32 as i32
}

/// Reads a number as LuaJIT's scanner does (`tonumber`, string coercion in arithmetic, the lexer).
///
/// Optional surrounding whitespace and sign; decimal digits with an optional fraction and exponent; `0x` hexadecimal
/// with an optional fraction and binary exponent (`0x1.8p3`); `0b` binary integers; `inf`, `infinity` and `nan` in any
/// case. Anything else, including an empty string, is `None`. Results are rounded once, to nearest, ties to even.
#[must_use]
pub fn parse(text: &[u8]) -> Option<f64> {
    let text = trim(text);
    let (negative, digits) = match text.first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let magnitude = parse_unsigned(digits)?;
    Some(if negative { -magnitude } else { magnitude })
}

fn trim(text: &[u8]) -> &[u8] {
    let space = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = text.iter().position(|c| !space(c)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|c| !space(c))
        .map_or(start, |e| e + 1);
    &text[start..end]
}

fn parse_unsigned(text: &[u8]) -> Option<f64> {
    if text.eq_ignore_ascii_case(b"inf") || text.eq_ignore_ascii_case(b"infinity") {
        return Some(f64::INFINITY);
    }
    if text.eq_ignore_ascii_case(b"nan") {
        return Some(f64::NAN);
    }
    match text {
        [b'0', b'x' | b'X', rest @ ..] => parse_hex(rest),
        [b'0', b'b' | b'B', rest @ ..] => parse_binary(rest),
        _ => parse_decimal(text),
    }
}

fn parse_decimal(text: &[u8]) -> Option<f64> {
    let mut at = 0;
    let digits = |at: &mut usize| {
        let start = *at;
        while text.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        *at - start
    };
    let mut mantissa = digits(&mut at);
    if text.get(at) == Some(&b'.') {
        at += 1;
        mantissa += digits(&mut at);
    }
    if mantissa == 0 {
        return None;
    }
    if matches!(text.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(text.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        if digits(&mut at) == 0 {
            return None;
        }
    }
    if at != text.len() {
        return None;
    }
    // The text is ASCII digits, a point and an exponent: Rust reads it correctly rounded.
    std::str::from_utf8(text).ok()?.parse().ok()
}

fn parse_binary(text: &[u8]) -> Option<f64> {
    if text.is_empty() || !text.iter().all(|c| matches!(c, b'0' | b'1')) {
        return None;
    }
    let start = text.iter().position(|&c| c == b'1').unwrap_or(text.len());
    let significant = &text[start..];
    let mut mantissa = 0u128;
    let mut exponent = 0i64;
    let mut sticky = false;
    for &c in significant {
        if mantissa >> 120 == 0 {
            mantissa = mantissa << 1 | u128::from(c - b'0');
        } else {
            exponent += 1;
            sticky |= c == b'1';
        }
    }
    Some(scale(mantissa, sticky, exponent))
}

fn parse_hex(text: &[u8]) -> Option<f64> {
    let mut mantissa = 0u128;
    let mut exponent = 0i64;
    let mut sticky = false;
    let mut digits = 0;
    let mut at = 0;
    let mut fraction = false;
    while let Some(&c) = text.get(at) {
        if c == b'.' && !fraction {
            fraction = true;
            at += 1;
            continue;
        }
        let Some(value) = (c as char).to_digit(16) else {
            break;
        };
        digits += 1;
        if mantissa >> 116 == 0 {
            mantissa = mantissa << 4 | u128::from(value);
            if fraction {
                exponent -= 4;
            }
        } else {
            sticky |= value != 0;
            if !fraction {
                exponent += 4;
            }
        }
        at += 1;
    }
    if digits == 0 {
        return None;
    }
    if matches!(text.get(at), Some(b'p' | b'P')) {
        at += 1;
        let negative = match text.get(at) {
            Some(b'-') => {
                at += 1;
                true
            }
            Some(b'+') => {
                at += 1;
                false
            }
            _ => false,
        };
        let start = at;
        let mut power = 0i64;
        while let Some(c) = text.get(at).filter(|c| c.is_ascii_digit()) {
            power = (power * 10 + i64::from(c - b'0')).min(1 << 20);
            at += 1;
        }
        if at == start {
            return None;
        }
        exponent += if negative { -power } else { power };
    }
    if at != text.len() {
        return None;
    }
    Some(scale(mantissa, sticky, exponent))
}

/// `mantissa * 2^exponent`, rounded once to nearest with ties to even; `sticky` says nonzero bits were dropped below
/// the mantissa.
fn scale(mantissa: u128, sticky: bool, exponent: i64) -> f64 {
    if mantissa == 0 {
        return 0.0;
    }
    let length = i64::from(128 - mantissa.leading_zeros());
    let top = length - 1 + exponent;
    if top > 1023 {
        return f64::INFINITY;
    }
    // Keep 53 bits, fewer when the result is subnormal; below half the smallest subnormal it is zero.
    let precision = if top < -1022 { 53 - (-1022 - top) } else { 53 };
    if precision < 0 {
        return 0.0;
    }
    let drop = (length - precision).max(0);
    let mut kept = if drop >= 128 { 0 } else { mantissa >> drop };
    if drop > 0 {
        let half = 1u128 << (drop - 1);
        let mask = if drop >= 128 {
            u128::MAX
        } else {
            (1u128 << drop) - 1
        };
        let rest = mantissa & mask;
        if rest > half || (rest == half && (sticky || kept & 1 == 1)) {
            kept += 1;
        }
    }
    let power = exponent + drop;
    let kept = kept as f64;
    // `kept` has at most 54 bits, so scaling by powers of two is exact unless it overflows.
    let first = power.clamp(-1022, 1023);
    let second = (power - first).clamp(-1022, 1023);
    kept * power_of_two(first) * power_of_two(second)
}

fn power_of_two(exponent: i64) -> f64 {
    f64::from_bits(((exponent + 1023) as u64) << 52)
}

/// The math library's functions beyond the basic operations: the platform's today, the pure-Rust `libm` crate once
/// script runs must match across platforms, by changing this module alone.
pub mod math {
    /// `a ^ b`.
    #[must_use]
    pub fn pow(a: f64, b: f64) -> f64 {
        a.powf(b)
    }
    /// `math.floor`.
    #[must_use]
    pub fn floor(a: f64) -> f64 {
        a.floor()
    }
    /// `math.ceil`.
    #[must_use]
    pub fn ceil(a: f64) -> f64 {
        a.ceil()
    }
    /// `math.sqrt`.
    #[must_use]
    pub fn sqrt(a: f64) -> f64 {
        a.sqrt()
    }
    /// `math.abs`.
    #[must_use]
    pub fn abs(a: f64) -> f64 {
        a.abs()
    }
    /// `math.sin`.
    #[must_use]
    pub fn sin(a: f64) -> f64 {
        a.sin()
    }
    /// `math.cos`.
    #[must_use]
    pub fn cos(a: f64) -> f64 {
        a.cos()
    }
    /// `math.tan`.
    #[must_use]
    pub fn tan(a: f64) -> f64 {
        a.tan()
    }
    /// `math.asin`.
    #[must_use]
    pub fn asin(a: f64) -> f64 {
        a.asin()
    }
    /// `math.acos`.
    #[must_use]
    pub fn acos(a: f64) -> f64 {
        a.acos()
    }
    /// `math.atan`.
    #[must_use]
    pub fn atan(a: f64) -> f64 {
        a.atan()
    }
    /// `math.atan2`.
    #[must_use]
    pub fn atan2(y: f64, x: f64) -> f64 {
        y.atan2(x)
    }
    /// `math.sinh`.
    #[must_use]
    pub fn sinh(a: f64) -> f64 {
        a.sinh()
    }
    /// `math.cosh`.
    #[must_use]
    pub fn cosh(a: f64) -> f64 {
        a.cosh()
    }
    /// `math.tanh`.
    #[must_use]
    pub fn tanh(a: f64) -> f64 {
        a.tanh()
    }
    /// `math.exp`.
    #[must_use]
    pub fn exp(a: f64) -> f64 {
        a.exp()
    }
    /// `math.log`.
    #[must_use]
    pub fn ln(a: f64) -> f64 {
        a.ln()
    }
    /// `math.log10`.
    #[must_use]
    pub fn log10(a: f64) -> f64 {
        a.log10()
    }
    /// `math.fmod`: the remainder with the dividend's sign, as C's `fmod`.
    #[must_use]
    pub fn fmod(a: f64, b: f64) -> f64 {
        a % b
    }
    /// `math.modf`: the integer part (toward zero) and the fraction; infinities have a zero fraction.
    #[must_use]
    pub fn modf(a: f64) -> (f64, f64) {
        let integer = a.trunc();
        // The fraction keeps the sign, as C's modf does: modf(-3) is -3 and -0.
        let fraction = if a.is_infinite() { 0.0 } else { a - integer };
        (integer, fraction.copysign(a))
    }
    /// `math.deg`.
    #[must_use]
    pub fn deg(a: f64) -> f64 {
        a * (180.0 / std::f64::consts::PI)
    }
    /// `math.rad`.
    #[must_use]
    pub fn rad(a: f64) -> f64 {
        a * (std::f64::consts::PI / 180.0)
    }
    /// `math.frexp`: a mantissa in [0.5, 1) and a power of two.
    #[must_use]
    pub fn frexp(a: f64) -> (f64, i32) {
        if a == 0.0 || a.is_nan() || a.is_infinite() {
            return (a, 0);
        }
        let bits = a.to_bits();
        let biased = ((bits >> 52) & 0x7ff) as i32;
        if biased == 0 {
            // Subnormal: scale into the normal range first.
            let (m, e) = frexp(a * 2f64.powi(64));
            return (m, e - 64);
        }
        let mantissa = f64::from_bits((bits & !(0x7ff << 52)) | (1022 << 52));
        (mantissa, biased - 1022)
    }
    /// `math.ldexp`: `m * 2^e`.
    #[must_use]
    pub fn ldexp(m: f64, e: i32) -> f64 {
        let e = e.clamp(-2200, 2200);
        let first = e.clamp(-1000, 1000);
        let second = (e - first).clamp(-1000, 1000);
        let third = e - first - second;
        m * 2f64.powi(first) * 2f64.powi(second) * 2f64.powi(third)
    }
    /// A double in [0, 1) from 53 random bits.
    #[must_use]
    pub fn unit(bits: u64) -> f64 {
        (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

#[cfg(test)]
mod tests;
