//! The string library, with LuaJIT's extensions (`rep`'s separator, 64-bit `%d` and `%x`), every copy paid in fuel
//! and bounded by the longest string a script may build. No `string.dump`.

use std::rc::Rc;

use super::base::MAX_RESULTS;
use super::pattern::{Capture, Matcher, PatternError, is_plain};
use crate::heap::Function;
use crate::number;
use crate::value::{StrId, Value};
use crate::vm::{Args, LuaError, Vm};

pub(super) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    let string = vm.library(
        "string",
        &[
            ("byte", byte),
            ("char", char),
            ("find", find),
            ("format", format),
            ("gmatch", gmatch),
            ("gsub", gsub),
            ("len", len),
            ("lower", lower),
            ("match", match_),
            ("rep", rep),
            ("reverse", reverse),
            ("sub", sub),
            ("upper", upper),
        ],
    )?;
    let meta = vm.heap.new_table(0, 1)?;
    vm.raw_set_checked(meta, Value::String(vm.names.index), Value::Table(string))?;
    vm.string_meta = Some(meta);
    Ok(())
}

/// Lua 5.1's relative string positions: negative counts from the end.
fn position(i: i64, length: usize) -> i64 {
    if i < 0 {
        (length as i64 + i + 1).max(0)
    } else {
        i
    }
}

fn bytes_of(vm: &mut Vm, args: Args, i: usize) -> Result<(StrId, Rc<[u8]>), LuaError> {
    let s = vm.check_string(args, i)?;
    Ok((s, vm.heap.string_rc(s)))
}

/// Charges the fuel for copying `length` bytes and checks the result's length.
fn charge_copy(vm: &mut Vm, length: usize) -> Result<(), LuaError> {
    if length > vm.limits.string_length {
        return Err(vm.string_overflow());
    }
    vm.charge_fuel(length as u64 / 16)
}

fn len(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    // LuaJIT's fast path sees a missing argument as nil.
    if args.count == 0 {
        return Err(vm.arg_error(args, 0, "string expected, got nil"));
    }
    let s = vm.check_string(args, 0)?;
    let length = vm.heap.string(s).len();
    vm.results(&[Value::Number(length as f64)])
}

fn sub(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, s) = bytes_of(vm, args, 0)?;
    let start = position(vm.check_integer(args, 1)?, s.len()).max(1);
    let end = position(vm.opt_integer(args, 2, -1)?, s.len()).min(s.len() as i64);
    if start > end {
        return vm.push_string(b"");
    }
    let slice = &s[start as usize - 1..end as usize];
    charge_copy(vm, slice.len())?;
    vm.push_string(slice)
}

fn upper(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, s) = bytes_of(vm, args, 0)?;
    charge_copy(vm, s.len())?;
    vm.push_string(&s.to_ascii_uppercase())
}

fn lower(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, s) = bytes_of(vm, args, 0)?;
    charge_copy(vm, s.len())?;
    vm.push_string(&s.to_ascii_lowercase())
}

fn reverse(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, s) = bytes_of(vm, args, 0)?;
    charge_copy(vm, s.len())?;
    let mut reversed = s.to_vec();
    reversed.reverse();
    vm.push_string(&reversed)
}

fn rep(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, s) = bytes_of(vm, args, 0)?;
    let count = vm.check_integer(args, 1)?;
    let separator = match vm.opt_string(args, 2)? {
        Some(sep) => vm.heap.string(sep).to_vec(),
        None => Vec::new(),
    };
    if count <= 0 || (s.is_empty() && separator.is_empty()) {
        return vm.push_string(b"");
    }
    let total =
        (s.len() as u128 + separator.len() as u128) * count as u128 - separator.len() as u128;
    if total > vm.limits.string_length as u128 {
        return Err(vm.string_overflow());
    }
    charge_copy(vm, total as usize)?;
    let mut out = Vec::with_capacity(total as usize);
    for i in 0..count {
        if i > 0 {
            out.extend_from_slice(&separator);
        }
        out.extend_from_slice(&s);
    }
    vm.push_string(&out)
}

fn byte(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, s) = bytes_of(vm, args, 0)?;
    let first = position(vm.opt_integer(args, 1, 1)?, s.len());
    let end = match vm.arg(args, 2) {
        Value::Nil => first,
        _ => position(vm.check_integer(args, 2)?, s.len()),
    }
    .min(s.len() as i64);
    let start = first.max(1);
    if start > end {
        return Ok(0);
    }
    let count = (end - start + 1) as usize;
    if count >= MAX_RESULTS {
        return Err(vm.error("string slice too long"));
    }
    vm.charge_fuel(count as u64)?;
    for &b in &s[start as usize - 1..end as usize] {
        vm.push(Value::Number(f64::from(b)))?;
    }
    Ok(count)
}

fn char(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let mut out = Vec::with_capacity(args.count);
    for i in 0..args.count {
        let c = vm.check_integer(args, i)?;
        let Ok(c) = u8::try_from(c) else {
            return Err(vm.arg_error(args, i, "invalid value"));
        };
        out.push(c);
    }
    vm.charge_fuel(out.len() as u64 / 16)?;
    vm.push_string(&out)
}

/// Where `needle` first occurs in `haystack`: Knuth-Morris-Pratt, linear in both, whatever the bytes.
fn search(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let mut failure = vec![0usize; needle.len()];
    let mut k = 0;
    for i in 1..needle.len() {
        while k > 0 && needle[i] != needle[k] {
            k = failure[k - 1];
        }
        if needle[i] == needle[k] {
            k += 1;
        }
        failure[i] = k;
    }
    let mut matched = 0;
    for (i, &c) in haystack.iter().enumerate() {
        while matched > 0 && c != needle[matched] {
            matched = failure[matched - 1];
        }
        if c == needle[matched] {
            matched += 1;
            if matched == needle.len() {
                return Some(i + 1 - needle.len());
            }
        }
    }
    None
}

// --- patterns ------------------------------------------------------------------------------------------------

fn pattern_error(vm: &mut Vm, error: PatternError) -> LuaError {
    error.into_lua(vm)
}

/// The value of capture `i` (or the whole match when the pattern has none).
fn capture_value(
    vm: &mut Vm,
    source: &[u8],
    captures: &[Capture],
    i: usize,
    start: usize,
    end: usize,
) -> Result<Value, LuaError> {
    if i >= captures.len() {
        if i == 0 {
            return vm.new_string(&source[start..end]);
        }
        return Err(vm.error("invalid capture index"));
    }
    match captures[i] {
        Capture::Span(s, l) => vm.new_string(&source[s..s + l]),
        Capture::Position(p) => Ok(Value::Number((p + 1) as f64)),
        Capture::Unfinished(_) => Err(vm.error("unfinished capture")),
    }
}

fn push_captures(
    vm: &mut Vm,
    source: &[u8],
    captures: &[Capture],
    start: usize,
    end: usize,
    whole: bool,
) -> Result<usize, LuaError> {
    let count = if captures.is_empty() && whole {
        1
    } else {
        captures.len()
    };
    for i in 0..count {
        let value = capture_value(vm, source, captures, i, start, end)?;
        vm.push(value)?;
    }
    Ok(count)
}

fn matcher<'a>(vm: &Vm, source: &'a [u8], pattern: &'a [u8]) -> Matcher<'a> {
    Matcher::new(
        source,
        pattern,
        vm.limits.pattern_depth,
        vm.fuel,
        vm.fuel_limit,
    )
}

fn find_aux(vm: &mut Vm, args: Args, find: bool) -> Result<usize, LuaError> {
    let (_, source) = bytes_of(vm, args, 0)?;
    let (_, pattern) = bytes_of(vm, args, 1)?;
    // LuaJIT starts past the end at the end: `("ab"):find("", 10)` finds the empty string at 3.
    let init = (position(vm.opt_integer(args, 2, 1)?, source.len()).max(1) - 1)
        .min(source.len() as i64) as usize;
    if find && (vm.arg(args, 3).is_truthy() || is_plain(&pattern)) {
        vm.charge_fuel((source.len().saturating_sub(init) + pattern.len()) as u64 / 16)?;
        let found = search(&source[init..], &pattern);
        return match found {
            Some(at) => vm.results(&[
                Value::Number((init + at + 1) as f64),
                Value::Number((init + at + pattern.len()) as f64),
            ]),
            None => vm.results(&[Value::Nil]),
        };
    }
    let anchored = pattern.first() == Some(&b'^');
    let p = usize::from(anchored);
    let mut m = matcher(vm, &source, &pattern);
    let mut s = init;
    let outcome = loop {
        match m.run(s, p) {
            Ok(Some(end)) => break Ok(Some((s, end))),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        s += 1;
        if anchored || s > source.len() {
            break Ok(None);
        }
    };
    vm.fuel = m.fuel;
    let captures = m.captures.clone();
    let check = m.check_captures();
    match outcome {
        Err(e) => Err(pattern_error(vm, e)),
        Ok(None) => vm.results(&[Value::Nil]),
        Ok(Some((start, end))) => {
            if let Err(e) = check {
                return Err(pattern_error(vm, e));
            }
            if find {
                vm.push(Value::Number((start + 1) as f64))?;
                vm.push(Value::Number(end as f64))?;
                Ok(2 + push_captures(vm, &source, &captures, start, end, false)?)
            } else {
                push_captures(vm, &source, &captures, start, end, true)
            }
        }
    }
}

fn find(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    find_aux(vm, args, true)
}

fn match_(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    find_aux(vm, args, false)
}

fn gmatch_step(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let Value::String(source) = vm.native_upvalue(args, 0) else {
        unreachable!("gmatch keeps its string")
    };
    let Value::String(pattern) = vm.native_upvalue(args, 1) else {
        unreachable!("gmatch keeps its pattern")
    };
    let Value::Number(at) = vm.native_upvalue(args, 2) else {
        unreachable!("gmatch keeps its position")
    };
    let source = vm.heap.string_rc(source);
    let pattern = vm.heap.string_rc(pattern);
    let mut s = at as usize;
    let mut m = matcher(vm, &source, &pattern);
    let outcome = loop {
        if s > source.len() {
            break Ok(None);
        }
        match m.run(s, 0) {
            Ok(Some(end)) => break Ok(Some((s, end))),
            Ok(None) => s += 1,
            Err(e) => break Err(e),
        }
    };
    vm.fuel = m.fuel;
    let captures = m.captures.clone();
    match outcome {
        Err(e) => Err(pattern_error(vm, e)),
        Ok(None) => {
            set_upvalue(vm, args, 2, Value::Number((source.len() + 1) as f64));
            Ok(0)
        }
        Ok(Some((start, end))) => {
            let next = if end == start { end + 1 } else { end };
            set_upvalue(vm, args, 2, Value::Number(next as f64));
            push_captures(vm, &source, &captures, start, end, true)
        }
    }
}

fn set_upvalue(vm: &mut Vm, args: Args, i: usize, value: Value) {
    if let Value::Function(f) = vm.stack[args.base - 1]
        && let Function::Native { upvalues, .. } = vm.heap.functions.get_mut(f.0)
    {
        upvalues[i] = value;
        vm.heap.barrier(value);
    }
}

fn gmatch(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let source = vm.check_string(args, 0)?;
    let pattern = vm.check_string(args, 1)?;
    let step = vm.native(
        "gmatch_aux",
        gmatch_step,
        vec![
            Value::String(source),
            Value::String(pattern),
            Value::Number(0.0),
        ],
    )?;
    vm.results(&[step])
}

fn gsub(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, source) = bytes_of(vm, args, 0)?;
    let (_, pattern) = bytes_of(vm, args, 1)?;
    let replacement = vm.arg(args, 2);
    let replacement = match replacement {
        Value::Number(_) => Value::String(vm.check_string(args, 2)?),
        Value::String(_) | Value::Table(_) | Value::Function(_) => replacement,
        _ => return Err(vm.arg_error(args, 2, "string/function/table expected")),
    };
    let max = match vm.arg(args, 3) {
        Value::Nil => i64::MAX,
        _ => vm.check_integer(args, 3)?,
    };
    let anchored = pattern.first() == Some(&b'^');
    let p = usize::from(anchored);
    let mut out = Vec::new();
    let mut s = 0;
    let mut count = 0i64;
    // The replacement value stays on the stack while Lua code runs, so the collector sees it.
    while count < max {
        let mut m = matcher(vm, &source, &pattern);
        let outcome = m.run(s, p);
        vm.fuel = m.fuel;
        let captures = m.captures.clone();
        let end = match outcome {
            Err(e) => return Err(pattern_error(vm, e)),
            Ok(end) => end,
        };
        if let Some(end) = end {
            count += 1;
            add_value(vm, &mut out, &source, &captures, s, end, replacement)?;
        }
        match end {
            Some(end) if end > s => s = end,
            _ if s < source.len() => {
                out.push(source[s]);
                s += 1;
            }
            _ => break,
        }
        if out.len() > vm.limits.string_length {
            return Err(vm.string_overflow());
        }
        vm.charge_fuel(1)?;
        if anchored {
            break;
        }
    }
    out.extend_from_slice(&source[s.min(source.len())..]);
    charge_copy(vm, out.len())?;
    let result = vm.new_string(&out)?;
    vm.results(&[result, Value::Number(count as f64)])
}

fn add_value(
    vm: &mut Vm,
    out: &mut Vec<u8>,
    source: &[u8],
    captures: &[Capture],
    start: usize,
    end: usize,
    replacement: Value,
) -> Result<(), LuaError> {
    let value = match replacement {
        Value::String(r) => {
            let r = vm.heap.string_rc(r);
            let mut i = 0;
            while i < r.len() {
                let c = r[i];
                i += 1;
                if c != b'%' {
                    out.push(c);
                    continue;
                }
                // A `%` that ends the replacement stands for the NUL that ends C's string, as in LuaJIT.
                let d = r.get(i).copied().unwrap_or(0);
                i += 1;
                if d.is_ascii_digit() {
                    let index = usize::from(d - b'0');
                    let value = if index == 0 {
                        vm.new_string(&source[start..end])?
                    } else {
                        capture_value(vm, source, captures, index - 1, start, end)?
                    };
                    append(vm, out, value)?;
                } else {
                    out.push(d);
                }
            }
            return Ok(());
        }
        Value::Table(t) => {
            let key = capture_value(vm, source, captures, 0, start, end)?;
            vm.index(Value::Table(t), key)?
        }
        _ => {
            let count = if captures.is_empty() {
                1
            } else {
                captures.len()
            };
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                values.push(capture_value(vm, source, captures, i, start, end)?);
            }
            vm.call_meta(replacement, &values)?
        }
    };
    match value {
        Value::Nil | Value::Boolean(false) => out.extend_from_slice(&source[start..end]),
        Value::String(_) | Value::Number(_) => append(vm, out, value)?,
        other => {
            return Err(vm.error(format!(
                "invalid replacement value (a {})",
                other.type_name()
            )));
        }
    }
    Ok(())
}

fn append(vm: &mut Vm, out: &mut Vec<u8>, value: Value) -> Result<(), LuaError> {
    match value {
        Value::String(s) => {
            let piece = vm.heap.string_rc(s);
            append_bytes(vm, out, &piece)
        }
        Value::Number(n) => append_bytes(vm, out, number::format(n).as_bytes()),
        _ => Ok(()),
    }
}

/// Appends to an output after paying for the bytes and checking the result's length: no work before the check.
fn append_bytes(vm: &mut Vm, out: &mut Vec<u8>, piece: &[u8]) -> Result<(), LuaError> {
    vm.charge_fuel(piece.len() as u64 / 16)?;
    if out.len() + piece.len() > vm.limits.string_length {
        return Err(vm.string_overflow());
    }
    out.extend_from_slice(piece);
    Ok(())
}

// --- format --------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    zero: bool,
    width: usize,
    precision: Option<usize>,
}

fn format(vm: &mut Vm, args: Args) -> Result<usize, LuaError> {
    let (_, template) = bytes_of(vm, args, 0)?;
    vm.charge_fuel(template.len() as u64 / 16)?;
    let mut out = Vec::new();
    let mut argument = 1;
    let mut i = 0;
    while i < template.len() {
        let c = template[i];
        i += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        if template.get(i) == Some(&b'%') {
            out.push(b'%');
            i += 1;
            continue;
        }
        let start = i;
        let mut spec = Spec::default();
        while let Some(&flag) = template.get(i).filter(|f| b"-+ #0".contains(f)) {
            match flag {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.alternate = true,
                _ => spec.zero = true,
            }
            i += 1;
        }
        let digits = |i: &mut usize| {
            let from = *i;
            while *i - from < 2 && template.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
            (from, *i)
        };
        let (from, to) = digits(&mut i);
        spec.width = std::str::from_utf8(&template[from..to])
            .ok()
            .and_then(|d| d.parse().ok())
            .unwrap_or(0);
        if template.get(i) == Some(&b'.') {
            i += 1;
            let (from, to) = digits(&mut i);
            spec.precision = Some(
                std::str::from_utf8(&template[from..to])
                    .ok()
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(0),
            );
        }
        let Some(&conversion) = template.get(i) else {
            let shown = crate::shown(&template[start - 1..i]);
            return Err(vm.error(format!("invalid option '{shown}' to 'format'")));
        };
        i += 1;
        if b"diuoxXceEfFgGaAqsp".contains(&conversion) && argument >= args.count {
            return Err(vm.arg_error(args, argument, "value expected"));
        }
        let text = match conversion {
            b'd' | b'i' => {
                let n = vm.check_number(args, argument)?;
                format_integer(number::to_int64(n), &spec)
            }
            b'u' => {
                let n = vm.check_number(args, argument)?;
                pad(&spec, String::new(), number::to_uint64(n).to_string(), true)
            }
            b'o' | b'x' | b'X' => {
                let n = vm.check_number(args, argument)?;
                format_unsigned(number::to_uint64(n), conversion, &spec)
            }
            b'c' => {
                let n = vm.check_number(args, argument)?;
                // LuaJIT converts through a 32-bit integer (x86-64's "integer indefinite" when out of range).
                let n = if (-2_147_483_648.0..2_147_483_648.0).contains(&n) {
                    n as i32
                } else {
                    i32::MIN
                };
                let mut text = vec![n as u8];
                pad_bytes(&spec, &mut text);
                argument += 1;
                append_bytes(vm, &mut out, &text)?;
                continue;
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                let n = vm.check_number(args, argument)?;
                format_float(n, conversion, &spec)
            }
            b'a' | b'A' => {
                let n = vm.check_number(args, argument)?;
                format_hex_float(n, conversion == b'A', &spec)
            }
            b'p' => vm.check_any(args, argument)?.pointer_identity(),
            b'q' => {
                let value = vm.check_any(args, argument)?;
                quote(vm, value, &mut out)?;
                argument += 1;
                continue;
            }
            b's' => {
                let value = vm.check_any(args, argument)?;
                let shown = format_value(vm, value)?;
                let mut text = match shown {
                    Value::String(s) => {
                        let piece = vm.heap.string_rc(s);
                        let kept = spec.precision.map_or(piece.len(), |p| p.min(piece.len()));
                        vm.charge_fuel(kept as u64 / 16)?;
                        if out.len() + kept > vm.limits.string_length {
                            return Err(vm.string_overflow());
                        }
                        piece[..kept].to_vec()
                    }
                    Value::Number(n) => number::format(n).into_bytes(),
                    _ => return Err(vm.arg_type_error(args, argument, "string")),
                };
                if let Some(precision) = spec.precision {
                    text.truncate(precision);
                }
                pad_bytes(&spec, &mut text);
                argument += 1;
                append_bytes(vm, &mut out, &text)?;
                continue;
            }
            other => {
                let shown = crate::shown(&template[start - 1..i]);
                let _ = other;
                return Err(vm.error(format!("invalid option '{shown}' to 'format'")));
            }
        };
        argument += 1;
        append_bytes(vm, &mut out, text.as_bytes())?;
    }
    vm.push_string(&out)
}

/// LuaJIT formats strings directly; other values get one `__tostring` call then default rendering.
fn format_value(vm: &mut Vm, value: Value) -> Result<Value, LuaError> {
    let shown = vm.display_value(value)?;
    vm.display_raw_value(shown)
}

fn pad_bytes(spec: &Spec, text: &mut Vec<u8>) {
    if text.len() >= spec.width {
        return;
    }
    let fill = vec![b' '; spec.width - text.len()];
    if spec.left {
        text.extend_from_slice(&fill);
    } else {
        text.splice(0..0, fill);
    }
}

/// Pads `sign` + `digits` to the width: zeros after the sign when asked (and `numeric`), else spaces.
fn pad(spec: &Spec, sign: String, digits: String, numeric: bool) -> String {
    let length = sign.len() + digits.len();
    if length >= spec.width {
        return sign + &digits;
    }
    let fill = spec.width - length;
    if spec.left {
        sign + &digits + &" ".repeat(fill)
    } else if spec.zero && numeric {
        sign + &"0".repeat(fill) + &digits
    } else {
        " ".repeat(fill) + &sign + &digits
    }
}

fn sign_of(negative: bool, spec: &Spec) -> String {
    if negative {
        "-".into()
    } else if spec.plus {
        "+".into()
    } else if spec.space {
        " ".into()
    } else {
        String::new()
    }
}

fn format_integer(n: i64, spec: &Spec) -> String {
    let mut digits = n.unsigned_abs().to_string();
    if let Some(precision) = spec.precision {
        if precision == 0 && n == 0 {
            digits.clear();
        }
        while digits.len() < precision {
            digits.insert(0, '0');
        }
    }
    let numeric = spec.precision.is_none();
    pad(spec, sign_of(n < 0, spec), digits, numeric)
}

fn format_unsigned(n: u64, conversion: u8, spec: &Spec) -> String {
    let mut digits = match conversion {
        b'o' => format!("{n:o}"),
        b'x' => format!("{n:x}"),
        _ => format!("{n:X}"),
    };
    if let Some(precision) = spec.precision {
        if precision == 0 && n == 0 {
            digits.clear();
        }
        while digits.len() < precision {
            digits.insert(0, '0');
        }
    }
    let prefix = match conversion {
        b'o' if spec.alternate && !digits.starts_with('0') => "0",
        b'x' if spec.alternate && n != 0 => "0x",
        b'X' if spec.alternate && n != 0 => "0X",
        _ => "",
    };
    pad(spec, prefix.into(), digits, spec.precision.is_none())
}

/// C's `%e`, `%f` and `%g` on a double: Rust's correctly rounded digits, laid out as C does.
fn format_float(n: f64, conversion: u8, spec: &Spec) -> String {
    let upper = conversion.is_ascii_uppercase();
    let negative = n.is_sign_negative() && !n.is_nan();
    let sign = if n.is_nan() {
        if spec.space {
            " ".into()
        } else {
            String::new()
        }
    } else {
        sign_of(negative, spec)
    };
    let magnitude = n.abs();
    if !n.is_finite() {
        let text = if n.is_nan() { "nan" } else { "inf" };
        let text = if upper {
            text.to_uppercase()
        } else {
            text.to_owned()
        };
        return pad(spec, sign, text, false);
    }
    let precision = spec.precision.unwrap_or(6);
    let body = match conversion.to_ascii_lowercase() {
        b'f' => {
            let mut text = format!("{magnitude:.precision$}");
            if spec.alternate && precision == 0 {
                text.push('.');
            }
            text
        }
        b'e' => exponent_form(magnitude, precision, spec.alternate),
        _ => {
            let precision = precision.max(1);
            let scientific = format!("{magnitude:.prec$e}", prec = precision - 1);
            let exponent: i32 = scientific
                .split_once('e')
                .map_or(0, |(_, e)| e.parse().unwrap_or(0));
            let mut text = if exponent < -4 || exponent >= precision as i32 {
                exponent_form(magnitude, precision - 1, spec.alternate)
            } else {
                let decimals = (precision as i32 - 1 - exponent) as usize;
                format!("{magnitude:.decimals$}")
            };
            if !spec.alternate {
                text = strip_zeros(&text);
            } else if !text.contains('.') {
                text.push('.');
            }
            text
        }
    };
    let body = if upper { body.to_uppercase() } else { body };
    pad(spec, sign, body, true)
}

/// `%a`, as LuaJIT writes it, from the bits alone: a subnormal normalized to a leading 1, the 13 hex digits of the
/// mantissa without trailing zeros (or rounded half up to the precision, a carry making the leading digit 2), and the
/// binary exponent in decimal.
fn format_hex_float(n: f64, upper: bool, spec: &Spec) -> String {
    const FRACTION: u64 = (1 << 52) - 1;
    if !n.is_finite() {
        return format_float(n, if upper { b'E' } else { b'e' }, spec);
    }
    let bits = n.to_bits();
    let sign = sign_of(bits >> 63 == 1, spec);
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & FRACTION;
    let (mut lead, mantissa, exponent) = match (biased, fraction) {
        (0, 0) => (0, 0, 0),
        (0, _) => {
            let top = 63 - fraction.leading_zeros() as i32;
            (1, (fraction << (52 - top)) & FRACTION, top - 1074)
        }
        _ => (1, fraction, biased - 1023),
    };
    let digits = match spec.precision {
        None => format!("{mantissa:013x}").trim_end_matches('0').to_owned(),
        Some(precision) if precision >= 13 => {
            format!("{mantissa:013x}{}", "0".repeat(precision - 13))
        }
        Some(precision) => {
            let shift = 4 * (13 - precision as u32);
            let mut rounded = (mantissa + (1 << (shift - 1))) >> shift;
            if rounded >> (4 * precision) != 0 {
                lead += 1;
                rounded &= (1 << (4 * precision)) - 1;
            }
            match precision {
                0 => String::new(),
                _ => format!("{rounded:0precision$x}"),
            }
        }
    };
    let point = if digits.is_empty() && !spec.alternate {
        ""
    } else {
        "."
    };
    let exponent_sign = if exponent < 0 { '-' } else { '+' };
    let body = format!(
        "{lead}{point}{digits}p{exponent_sign}{}",
        exponent.unsigned_abs()
    );
    let (prefix, body) = if upper {
        ("0X", body.to_uppercase())
    } else {
        ("0x", body)
    };
    pad(spec, sign + prefix, body, true)
}

fn exponent_form(magnitude: f64, precision: usize, alternate: bool) -> String {
    let scientific = format!("{magnitude:.precision$e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust's exponent form has an 'e'");
    let exponent: i32 = exponent.parse().expect("Rust's exponent is an integer");
    let mut mantissa = mantissa.to_owned();
    if alternate && !mantissa.contains('.') {
        mantissa.push('.');
    }
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
}

/// Drops trailing fraction zeros (and the point) from `%g`'s output, keeping any exponent.
fn strip_zeros(text: &str) -> String {
    let (mantissa, exponent) = match text.find('e') {
        Some(e) => (&text[..e], &text[e..]),
        None => (text, ""),
    };
    let mantissa = if mantissa.contains('.') {
        mantissa.trim_end_matches('0').trim_end_matches('.')
    } else {
        mantissa
    };
    format!("{mantissa}{exponent}")
}

/// `%q`: quote LuaJIT's string rendering, including non-strings, after checking the full escaped size.
fn quote(vm: &mut Vm, value: Value, out: &mut Vec<u8>) -> Result<(), LuaError> {
    let shown = format_value(vm, value)?;
    let Value::String(id) = shown else {
        return Err(vm.error("string expected"));
    };
    let bytes = vm.heap.string_rc(id);
    vm.charge_fuel(1 + bytes.len() as u64 / 16)?;
    let mut length = 2usize;
    for (i, &byte) in bytes.iter().enumerate() {
        length = length.saturating_add(quoted_byte(byte, bytes.get(i + 1)).1);
    }
    if out.len().saturating_add(length) > vm.limits.string_length {
        return Err(vm.string_overflow());
    }
    vm.charge_fuel(1 + length as u64 / 16)?;
    out.push(b'"');
    for (i, &byte) in bytes.iter().enumerate() {
        let (escaped, length) = quoted_byte(byte, bytes.get(i + 1));
        out.extend_from_slice(&escaped[..length]);
    }
    out.push(b'"');
    Ok(())
}

fn quoted_byte(byte: u8, next: Option<&u8>) -> ([u8; 4], usize) {
    match byte {
        b'"' | b'\\' | b'\n' => ([b'\\', byte, 0, 0], 2),
        0 if !next.is_some_and(u8::is_ascii_digit) => ([b'\\', b'0', 0, 0], 2),
        mut c if c < 0x20 || c == 0x7f => {
            let digits = if next.is_some_and(u8::is_ascii_digit) || c >= 100 {
                3
            } else if c >= 10 {
                2
            } else {
                1
            };
            let mut escaped = [b'\\', 0, 0, 0];
            for index in (1..=digits).rev() {
                escaped[index] = b'0' + c % 10;
                c /= 10;
            }
            (escaped, digits + 1)
        }
        c => ([c, 0, 0, 0], 1),
    }
}
