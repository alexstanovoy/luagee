//! Lua 5.1's patterns, as `lstrlib.c` matches them, with every step paid in fuel and the recursion bounded.

use crate::Limit;
use crate::vm::LuaError;

/// The most captures a pattern may have (Lua's `LUA_MAXCAPTURES`).
pub(crate) const MAX_CAPTURES: usize = 32;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Capture {
    /// A substring: start and length.
    Span(usize, usize),
    /// `()`: a position.
    Position(usize),
    Unfinished(usize),
}

/// A pattern error: a message for the caller to raise, or a limit.
pub(crate) enum PatternError {
    Message(&'static str),
    Limit(Limit),
}

impl PatternError {
    pub(crate) fn into_lua(self, vm: &mut crate::vm::Vm) -> LuaError {
        match self {
            PatternError::Message(m) => vm.error(m),
            PatternError::Limit(l) => LuaError::Limit(l),
        }
    }
}

pub(crate) struct Matcher<'a> {
    pub(crate) source: &'a [u8],
    pattern: &'a [u8],
    pub(crate) captures: Vec<Capture>,
    depth: u32,
    max_depth: u32,
    pub(crate) fuel: u64,
    fuel_limit: Limit,
}

type Step = Result<Option<usize>, PatternError>;

fn class_end(pattern: &[u8], mut p: usize) -> Result<usize, PatternError> {
    let c = pattern[p];
    p += 1;
    if c == b'%' {
        if p >= pattern.len() {
            return Err(PatternError::Message("malformed pattern (ends with '%')"));
        }
        return Ok(p + 1);
    }
    if c == b'[' {
        if pattern.get(p) == Some(&b'^') {
            p += 1;
        }
        // Look for the closing ']'; the first character may be one.
        loop {
            if p >= pattern.len() {
                return Err(PatternError::Message("malformed pattern (missing ']')"));
            }
            let c = pattern[p];
            p += 1;
            if c == b'%' {
                if p >= pattern.len() {
                    return Err(PatternError::Message("malformed pattern (missing ']')"));
                }
                p += 1;
            }
            if pattern.get(p) == Some(&b']') {
                return Ok(p + 1);
            }
        }
    }
    Ok(p)
}

fn single_class(c: u8, class: u8) -> bool {
    let matched = match class.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0,
        _ => return class == c,
    };
    if class.is_ascii_uppercase() {
        !matched
    } else {
        matched
    }
}

/// Whether `c` is in the set `pattern[p..end]` (from its '[' to its ']').
fn in_set(c: u8, pattern: &[u8], p: usize, end: usize) -> bool {
    let mut p = p + 1;
    let mut negate = false;
    if pattern[p] == b'^' {
        negate = true;
        p += 1;
    }
    let last = end - 1;
    while p < last {
        if pattern[p] == b'%' {
            p += 1;
            if single_class(c, pattern[p]) {
                return !negate;
            }
            p += 1;
        } else if pattern.get(p + 1) == Some(&b'-') && p + 2 < last {
            if pattern[p] <= c && c <= pattern[p + 2] {
                return !negate;
            }
            p += 3;
        } else {
            if pattern[p] == c {
                return !negate;
            }
            p += 1;
        }
    }
    negate
}

fn single_match(c: Option<u8>, pattern: &[u8], p: usize, end: usize) -> bool {
    let Some(c) = c else { return false };
    match pattern[p] {
        b'.' => true,
        b'%' => single_class(c, pattern[p + 1]),
        b'[' => in_set(c, pattern, p, end),
        other => other == c,
    }
}

impl<'a> Matcher<'a> {
    pub(crate) fn new(
        source: &'a [u8],
        pattern: &'a [u8],
        max_depth: u32,
        fuel: u64,
        fuel_limit: Limit,
    ) -> Matcher<'a> {
        Matcher {
            source,
            pattern,
            captures: Vec::new(),
            depth: 0,
            max_depth,
            fuel,
            fuel_limit,
        }
    }

    fn spend_many(&mut self, amount: u64) -> Result<(), PatternError> {
        if self.fuel < amount {
            self.fuel = 0;
            return Err(PatternError::Limit(self.fuel_limit));
        }
        self.fuel -= amount;
        Ok(())
    }

    fn spend(&mut self) -> Result<(), PatternError> {
        if self.fuel == 0 {
            return Err(PatternError::Limit(self.fuel_limit));
        }
        self.fuel -= 1;
        Ok(())
    }

    /// Tries to match the pattern from `p` against the source from `s`; the end of the match, if any.
    pub(crate) fn run(&mut self, s: usize, p: usize) -> Step {
        self.captures.clear();
        self.do_match(s, p)
    }

    fn do_match(&mut self, s: usize, p: usize) -> Step {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(PatternError::Message("pattern too complex"));
        }
        let result = self.match_loop(s, p);
        self.depth -= 1;
        result
    }

    fn match_loop(&mut self, mut s: usize, mut p: usize) -> Step {
        loop {
            self.spend()?;
            if p >= self.pattern.len() {
                return Ok(Some(s));
            }
            match self.pattern[p] {
                b'(' => {
                    return if self.pattern.get(p + 1) == Some(&b')') {
                        self.start_capture(s, p + 2, true)
                    } else {
                        self.start_capture(s, p + 1, false)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                b'$' if p + 1 == self.pattern.len() => {
                    return Ok((s == self.source.len()).then_some(s));
                }
                b'%' if self.pattern.get(p + 1) == Some(&b'b') => {
                    let Some(end) = self.balance(s, p + 2)? else {
                        return Ok(None);
                    };
                    s = end;
                    p += 4;
                    continue;
                }
                b'%' if self.pattern.get(p + 1) == Some(&b'f') => {
                    p += 2;
                    if self.pattern.get(p) != Some(&b'[') {
                        return Err(PatternError::Message("missing '[' after '%f' in pattern"));
                    }
                    let end = class_end(self.pattern, p)?;
                    let previous = if s == 0 { 0 } else { self.source[s - 1] };
                    let current = self.source.get(s).copied().unwrap_or(0);
                    if !in_set(previous, self.pattern, p, end)
                        && in_set(current, self.pattern, p, end)
                    {
                        p = end;
                        continue;
                    }
                    return Ok(None);
                }
                b'%' if self.pattern.get(p + 1).is_some_and(u8::is_ascii_digit) => {
                    let Some(end) = self.match_capture(s, self.pattern[p + 1])? else {
                        return Ok(None);
                    };
                    s = end;
                    p += 2;
                    continue;
                }
                _ => {}
            }
            let end = class_end(self.pattern, p)?;
            let current = self.source.get(s).copied();
            let matched = single_match(current, self.pattern, p, end);
            match self.pattern.get(end) {
                Some(b'?') => {
                    if matched && let Some(result) = self.do_match(s + 1, end + 1)? {
                        return Ok(Some(result));
                    }
                    p = end + 1;
                }
                Some(b'*') => return self.max_expand(s, p, end),
                Some(b'+') => {
                    return if matched {
                        self.max_expand(s + 1, p, end)
                    } else {
                        Ok(None)
                    };
                }
                Some(b'-') => return self.min_expand(s, p, end),
                _ => {
                    if !matched {
                        return Ok(None);
                    }
                    s += 1;
                    p = end;
                }
            }
        }
    }

    fn max_expand(&mut self, s: usize, p: usize, end: usize) -> Step {
        let mut count = 0;
        while single_match(self.source.get(s + count).copied(), self.pattern, p, end) {
            self.spend()?;
            count += 1;
        }
        loop {
            if let Some(result) = self.do_match(s + count, end + 1)? {
                return Ok(Some(result));
            }
            if count == 0 {
                return Ok(None);
            }
            count -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, end: usize) -> Step {
        loop {
            if let Some(result) = self.do_match(s, end + 1)? {
                return Ok(Some(result));
            }
            if single_match(self.source.get(s).copied(), self.pattern, p, end) {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, position: bool) -> Step {
        if self.captures.len() >= MAX_CAPTURES {
            return Err(PatternError::Message("too many captures"));
        }
        self.captures.push(if position {
            Capture::Position(s)
        } else {
            Capture::Unfinished(s)
        });
        let result = self.do_match(s, p)?;
        if result.is_none() {
            self.captures.pop();
        }
        Ok(result)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> Step {
        let Some(open) = self
            .captures
            .iter()
            .rposition(|c| matches!(c, Capture::Unfinished(_)))
        else {
            return Err(PatternError::Message("invalid pattern capture"));
        };
        let Capture::Unfinished(start) = self.captures[open] else {
            unreachable!()
        };
        self.captures[open] = Capture::Span(start, s - start);
        let result = self.do_match(s, p)?;
        if result.is_none() {
            self.captures[open] = Capture::Unfinished(start);
        }
        Ok(result)
    }

    fn balance(&mut self, s: usize, p: usize) -> Step {
        if p + 1 >= self.pattern.len() {
            return Err(PatternError::Message("unbalanced pattern"));
        }
        let (open, close) = (self.pattern[p], self.pattern[p + 1]);
        if self.source.get(s) != Some(&open) {
            return Ok(None);
        }
        let mut depth = 1;
        let mut at = s + 1;
        while at < self.source.len() {
            self.spend()?;
            let c = self.source[at];
            if c == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(at + 1));
                }
            } else if c == open {
                depth += 1;
            }
            at += 1;
        }
        Ok(None)
    }

    fn match_capture(&mut self, s: usize, digit: u8) -> Step {
        let index = usize::from(digit - b'0');
        let Some(Capture::Span(start, length)) = index
            .checked_sub(1)
            .and_then(|i| self.captures.get(i))
            .copied()
        else {
            return Err(PatternError::Message("invalid capture index"));
        };
        let end = s + length;
        if end > self.source.len() {
            return Ok(None);
        }
        // The comparison is paid before it runs, whether it matches or not.
        self.spend_many(1 + length as u64 / 16)?;
        if self.source[start..start + length] == self.source[s..end] {
            Ok(Some(end))
        } else {
            Ok(None)
        }
    }

    /// Checks no capture is left open after a match.
    pub(crate) fn check_captures(&self) -> Result<(), PatternError> {
        if self
            .captures
            .iter()
            .any(|c| matches!(c, Capture::Unfinished(_)))
        {
            return Err(PatternError::Message("unfinished capture"));
        }
        Ok(())
    }
}

/// Whether a pattern has no special characters (then `find` searches plainly).
pub(crate) fn is_plain(pattern: &[u8]) -> bool {
    !pattern.iter().any(|c| b"^$*+?.([%-".contains(c))
}
