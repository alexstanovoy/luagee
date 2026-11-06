//! The library functions that parse or build from script data, called on hostile arguments: format strings,
//! patterns and their subjects, number conversions in every base, `table.sort` with comparators that lie, and the
//! index arithmetic of `string.sub`, `select`, `unpack` and friends at the edges of the number line. Each call runs
//! with the checks of `run_bytes`.
#![no_main]

use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use luagee_fuzz::check_run;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    if let Ok(source) = call(&mut u) {
        check_run(source.as_bytes());
    }
});

fn call(u: &mut Unstructured) -> Result<String> {
    let source = match u.int_in_range(0..=9)? {
        0 => format!("return string.format({}, {}, {}, {})", format_string(u)?, value(u)?, value(u)?, value(u)?),
        1 => {
            let function = u.choose(&["find", "match", "gmatch", "gsub"])?;
            let (subject, pattern) = (string(u)?, pattern(u)?);
            match *function {
                "gmatch" => format!("local n = 0 for a, b in string.gmatch({subject}, {pattern}) do n = n + 1 end return n"),
                "gsub" => format!("return string.gsub({subject}, {pattern}, {}, {})", replacement(u)?, number(u)?),
                other => format!("return string.{other}({subject}, {pattern}, {}, {})", number(u)?, u.arbitrary::<bool>()?),
            }
        }
        2 => format!("return string.rep({}, {}, {})", string(u)?, number(u)?, string(u)?),
        3 => format!("return tonumber({}, {})", string(u)?, number(u)?),
        4 => format!("return tostring({}), tonumber(tostring({}))", number(u)?, number(u)?),
        5 => {
            let items: Vec<String> = (0..u.int_in_range(0..=40)?).map(|_| value(u)).collect::<Result<_>>()?;
            let lies = string(u)?;
            format!(
                "local t = {{{}}} local bits, i = {lies}, 0 \
                 local ok, e = pcall(table.sort, t, function(a, b) i = i + 1 \
                 return #bits > 0 and bits:byte(i % #bits + 1) % 2 == 1 end) return ok, #t",
                items.join(", ")
            )
        }
        6 => {
            let function = u.choose(&["sub", "byte", "char"])?;
            format!("return string.{function}({}, {}, {})", string(u)?, number(u)?, number(u)?)
        }
        7 => format!("return select({}, 1, 2, 3)", number(u)?),
        8 => format!("return unpack({{1, 2, 3}}, {}, {})", number(u)?, number(u)?),
        _ => {
            let function = u.choose(&[
                "bit.band", "bit.lshift", "bit.rshift", "bit.arshift", "bit.rol", "bit.tohex", "bit.tobit",
                "math.floor", "math.fmod", "math.modf", "math.frexp", "math.ldexp", "math.pow", "math.random",
                "string.char", "table.insert", "table.remove",
            ])?;
            let target = if function.starts_with("table.") { "{1, 2, 3}".to_owned() } else { number(u)? };
            format!("local t = {target} return {function}(t, {})", number(u)?)
        }
    };
    Ok(source)
}

/// A Lua string literal of arbitrary bytes.
fn literal(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in bytes {
        out.push_str(&format!("\\{b}"));
    }
    out.push('"');
    out
}

fn string(u: &mut Unstructured) -> Result<String> {
    let length = u.int_in_range(0..=24)?;
    Ok(literal(u.bytes(length.min(u.len()))?))
}

/// Mostly the bytes patterns are made of.
fn pattern(u: &mut Unstructured) -> Result<String> {
    const PIECES: [&[u8]; 24] = [
        b"%", b"%a", b"%d", b"%s", b"%w", b"%b()", b"%f[%w]", b"%1", b"%2", b"(", b")", b"()", b"[", b"]", b"[^", b"-",
        b"*", b"+", b"?", b".", b"^", b"$", b"a", b"%z",
    ];
    let mut bytes = Vec::new();
    for _ in 0..u.int_in_range(0..=12)? {
        if u.ratio(1, 6)? {
            bytes.push(u.arbitrary()?);
        } else {
            bytes.extend_from_slice(u.choose(&PIECES)?);
        }
    }
    Ok(literal(&bytes))
}

/// Mostly the bytes format strings are made of.
fn format_string(u: &mut Unstructured) -> Result<String> {
    const PIECES: [&[u8]; 20] = [
        b"%", b"%d", b"%5.2f", b"%s", b"%q", b"%x", b"%X", b"%o", b"%c", b"%e", b"%g", b"%i", b"%u", b"%-+ #0",
        b"99", b".", b"%%", b"%10.3s", b"%a", b"x",
    ];
    let mut bytes = Vec::new();
    for _ in 0..u.int_in_range(0..=6)? {
        if u.ratio(1, 6)? {
            bytes.push(u.arbitrary()?);
        } else {
            bytes.extend_from_slice(u.choose(&PIECES)?);
        }
    }
    Ok(literal(&bytes))
}

fn replacement(u: &mut Unstructured) -> Result<String> {
    Ok(match u.int_in_range(0..=3)? {
        0 => string(u)?,
        1 => literal(b"%0%1-%2%%"),
        2 => "{a = 1, [\"1\"] = \"x\"}".to_owned(),
        _ => "function(a, b) return b end".to_owned(),
    })
}

/// A number literal: the edges of the number line, or any double.
fn number(u: &mut Unstructured) -> Result<String> {
    const EDGES: [&str; 18] = [
        "0", "1", "-1", "2", "36", "37", "-0", "0.5", "2^31", "2^32", "-2^31", "2^53", "2^63", "-2^63", "2^64",
        "1e308", "(1/0)", "(0/0)",
    ];
    if u.ratio(2, 3)? {
        return Ok((*u.choose(&EDGES)?).to_owned());
    }
    let n = f64::from_bits(u.arbitrary()?);
    Ok(if n.is_nan() {
        "(0/0)".to_owned()
    } else if n.is_infinite() {
        if n > 0.0 { "(1/0)".to_owned() } else { "(-1/0)".to_owned() }
    } else {
        format!("({n:e})")
    })
}

fn value(u: &mut Unstructured) -> Result<String> {
    Ok(match u.int_in_range(0..=3)? {
        0 => string(u)?,
        1 => "nil".to_owned(),
        2 => "{}".to_owned(),
        _ => number(u)?,
    })
}
