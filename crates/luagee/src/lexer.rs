//! Source text to tokens, as LuaJIT reads Lua 5.1: its number syntax, its string escapes (`\x`, `\z`, `\u{...}`)
//! and `goto`.

use crate::number;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Token {
    Name(Box<[u8]>),
    String(Box<[u8]>),
    Number(f64),
    And,
    Break,
    Do,
    Else,
    Elseif,
    End,
    False,
    For,
    Function,
    If,
    In,
    Local,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Then,
    True,
    Until,
    While,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Hash,
    Equal,
    NotEqual,
    LessEqual,
    GreaterEqual,
    Less,
    Greater,
    Assign,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    DoubleColon,
    Semicolon,
    Colon,
    Comma,
    Dot,
    Concat,
    Dots,
    Eof,
}

/// A syntax error: the line and Lua's words.
#[derive(Debug)]
pub(crate) struct SyntaxError {
    pub(crate) line: u32,
    pub(crate) message: String,
}

pub(crate) struct Lexer<'a> {
    source: &'a [u8],
    at: usize,
    pub(crate) line: u32,
    /// The longest string literal the lexer may build.
    max_string: usize,
}

fn keyword(name: &[u8]) -> Option<Token> {
    Some(match name {
        b"and" => Token::And,
        b"break" => Token::Break,
        b"do" => Token::Do,
        b"else" => Token::Else,
        b"elseif" => Token::Elseif,
        b"end" => Token::End,
        b"false" => Token::False,
        b"for" => Token::For,
        b"function" => Token::Function,
        b"if" => Token::If,
        b"in" => Token::In,
        b"local" => Token::Local,
        b"nil" => Token::Nil,
        b"not" => Token::Not,
        b"or" => Token::Or,
        b"repeat" => Token::Repeat,
        b"return" => Token::Return,
        b"then" => Token::Then,
        b"true" => Token::True,
        b"until" => Token::Until,
        b"while" => Token::While,
        _ => return None,
    })
}

/// LuaJIT's identifier characters: ASCII letters, digits, `_`, and every byte from 0x80 (UTF-8 in names).
fn is_name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(source: &'a [u8], max_string: usize) -> Lexer<'a> {
        // A first line starting with "#!" (a shebang) is skipped, as LuaJIT skips it.
        let at = if source.starts_with(b"#!") {
            source
                .iter()
                .position(|&c| c == b'\n')
                .unwrap_or(source.len())
        } else {
            0
        };
        Lexer {
            source,
            at,
            line: 1,
            max_string,
        }
    }

    fn peek(&self) -> u8 {
        self.source.get(self.at).copied().unwrap_or(0)
    }

    fn peek_at(&self, offset: usize) -> u8 {
        self.source.get(self.at + offset).copied().unwrap_or(0)
    }

    fn at_end(&self) -> bool {
        self.at >= self.source.len()
    }

    fn error(&self, message: impl Into<String>) -> SyntaxError {
        SyntaxError {
            line: self.line,
            message: message.into(),
        }
    }

    /// Consumes a newline (`\n`, `\r`, `\n\r` or `\r\n`) and counts the line.
    fn newline(&mut self) {
        let first = self.peek();
        self.at += 1;
        let second = self.peek();
        if (second == b'\n' || second == b'\r') && second != first {
            self.at += 1;
        }
        self.line += 1;
    }

    pub(crate) fn next_token(&mut self) -> Result<Token, SyntaxError> {
        loop {
            if self.at_end() {
                return Ok(Token::Eof);
            }
            let c = self.peek();
            match c {
                b'\n' | b'\r' => self.newline(),
                b' ' | b'\t' | 0x0b | 0x0c => self.at += 1,
                b'-' if self.peek_at(1) == b'-' => {
                    self.at += 2;
                    if self.peek() == b'['
                        && let Some(level) = self.long_bracket_level()
                    {
                        self.long_string(level, false)?;
                        continue;
                    }
                    while !self.at_end() && self.peek() != b'\n' && self.peek() != b'\r' {
                        self.at += 1;
                    }
                }
                b'[' => {
                    return match self.long_bracket_level() {
                        Some(level) => Ok(Token::String(self.long_string(level, true)?.into())),
                        None if self.peek_at(1) == b'=' => {
                            Err(self.error("invalid long string delimiter"))
                        }
                        None => {
                            self.at += 1;
                            Ok(Token::LeftBracket)
                        }
                    };
                }
                b'"' | b'\'' => return self.short_string(c).map(|s| Token::String(s.into())),
                b'0'..=b'9' => return self.number(),
                b'.' if self.peek_at(1).is_ascii_digit() => return self.number(),
                c if is_name(c) => {
                    let start = self.at;
                    while is_name(self.peek()) && !self.at_end() {
                        self.at += 1;
                    }
                    let name = &self.source[start..self.at];
                    return Ok(keyword(name).unwrap_or_else(|| Token::Name(name.into())));
                }
                _ => return self.symbol(),
            }
        }
    }

    fn symbol(&mut self) -> Result<Token, SyntaxError> {
        let c = self.peek();
        let next = self.peek_at(1);
        let (token, length) = match (c, next) {
            (b'=', b'=') => (Token::Equal, 2),
            (b'~', b'=') => (Token::NotEqual, 2),
            (b'<', b'=') => (Token::LessEqual, 2),
            (b'>', b'=') => (Token::GreaterEqual, 2),
            (b':', b':') => (Token::DoubleColon, 2),
            (b'.', b'.') if self.peek_at(2) == b'.' => (Token::Dots, 3),
            (b'.', b'.') => (Token::Concat, 2),
            (b'+', _) => (Token::Plus, 1),
            (b'-', _) => (Token::Minus, 1),
            (b'*', _) => (Token::Star, 1),
            (b'/', _) => (Token::Slash, 1),
            (b'%', _) => (Token::Percent, 1),
            (b'^', _) => (Token::Caret, 1),
            (b'#', _) => (Token::Hash, 1),
            (b'<', _) => (Token::Less, 1),
            (b'>', _) => (Token::Greater, 1),
            (b'=', _) => (Token::Assign, 1),
            (b'(', _) => (Token::LeftParen, 1),
            (b')', _) => (Token::RightParen, 1),
            (b'{', _) => (Token::LeftBrace, 1),
            (b'}', _) => (Token::RightBrace, 1),
            (b']', _) => (Token::RightBracket, 1),
            (b';', _) => (Token::Semicolon, 1),
            (b':', _) => (Token::Colon, 1),
            (b',', _) => (Token::Comma, 1),
            (b'.', _) => (Token::Dot, 1),
            _ => return Err(self.error(format!("unexpected symbol near '{}'", c.escape_ascii()))),
        };
        self.at += length;
        Ok(token)
    }

    /// At `[`: the level of a long bracket `[==[` (and consumes it), or None (consuming nothing).
    fn long_bracket_level(&mut self) -> Option<usize> {
        let mut level = 0;
        while self.peek_at(1 + level) == b'=' {
            level += 1;
        }
        if self.peek_at(1 + level) == b'[' {
            self.at += level + 2;
            Some(level)
        } else {
            None
        }
    }

    fn long_string(&mut self, level: usize, keep: bool) -> Result<Vec<u8>, SyntaxError> {
        let start_line = self.line;
        let mut out = Vec::new();
        if self.peek() == b'\n' || self.peek() == b'\r' {
            self.newline();
        }
        loop {
            if self.at_end() {
                let what = if keep { "string" } else { "comment" };
                return Err(SyntaxError {
                    line: self.line,
                    message: format!(
                        "unfinished long {what} (starting at line {start_line}) near '<eof>'"
                    ),
                });
            }
            let c = self.peek();
            if c == b']'
                && (0..level).all(|i| self.peek_at(1 + i) == b'=')
                && self.peek_at(1 + level) == b']'
            {
                self.at += level + 2;
                return Ok(out);
            }
            if c == b'\n' || c == b'\r' {
                self.newline();
                if keep {
                    out.push(b'\n');
                }
            } else {
                self.at += 1;
                if keep {
                    out.push(c);
                }
            }
            if out.len() > self.max_string {
                return Err(self.error("string too long"));
            }
        }
    }

    fn short_string(&mut self, quote: u8) -> Result<Vec<u8>, SyntaxError> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            if out.len() > self.max_string {
                return Err(self.error("string too long"));
            }
            if self.at_end() {
                return Err(self.error("unfinished string near '<eof>'"));
            }
            let c = self.peek();
            match c {
                c if c == quote => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\n' | b'\r' => return Err(self.error("unfinished string")),
                b'\\' => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                _ => {
                    out.push(c);
                    self.at += 1;
                }
            }
        }
    }

    fn escape(&mut self, out: &mut Vec<u8>) -> Result<(), SyntaxError> {
        let c = self.peek();
        let simple = match c {
            b'a' => Some(7),
            b'b' => Some(8),
            b'f' => Some(12),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(11),
            b'\\' => Some(b'\\'),
            b'"' => Some(b'"'),
            b'\'' => Some(b'\''),
            _ => None,
        };
        if let Some(byte) = simple {
            out.push(byte);
            self.at += 1;
            return Ok(());
        }
        match c {
            b'\n' | b'\r' => {
                self.newline();
                out.push(b'\n');
            }
            b'x' => {
                self.at += 1;
                let mut value = 0u32;
                for _ in 0..2 {
                    let digit = (self.peek() as char)
                        .to_digit(16)
                        .ok_or_else(|| self.error("invalid escape sequence"))?;
                    value = value * 16 + digit;
                    self.at += 1;
                }
                out.push(value as u8);
            }
            b'z' => {
                self.at += 1;
                while !self.at_end() {
                    match self.peek() {
                        b'\n' | b'\r' => self.newline(),
                        b' ' | b'\t' | 0x0b | 0x0c => self.at += 1,
                        _ => break,
                    }
                }
            }
            b'u' if self.peek_at(1) == b'{' => {
                self.at += 2;
                let mut code = 0u32;
                let mut digits = 0;
                while let Some(digit) = (self.peek() as char).to_digit(16) {
                    code = code.saturating_mul(16).saturating_add(digit);
                    digits += 1;
                    self.at += 1;
                }
                if digits == 0 || self.peek() != b'}' || code > 0x7fff_ffff {
                    return Err(self.error("invalid escape sequence"));
                }
                self.at += 1;
                utf8(code, out);
            }
            b'0'..=b'9' => {
                let mut value = 0u32;
                for _ in 0..3 {
                    if !self.peek().is_ascii_digit() {
                        break;
                    }
                    value = value * 10 + u32::from(self.peek() - b'0');
                    self.at += 1;
                }
                if value > 255 {
                    return Err(self.error("invalid escape sequence"));
                }
                out.push(value as u8);
            }
            _ if self.at_end() => return Err(self.error("unfinished string near '<eof>'")),
            _ => return Err(self.error("invalid escape sequence")),
        }
        Ok(())
    }

    fn number(&mut self) -> Result<Token, SyntaxError> {
        let start = self.at;
        // LuaJIT's lexer takes every character that can continue a number, then the scanner judges the whole.
        let hex = self.peek() == b'0' && matches!(self.peek_at(1), b'x' | b'X');
        loop {
            let c = self.peek();
            let exponent = if hex { b"pP" } else { b"eE" };
            let signed_exponent = (c == b'+' || c == b'-')
                && self.at > start
                && exponent.contains(&self.source[self.at - 1]);
            if !(signed_exponent || ((is_name(c) || c == b'.') && !self.at_end())) {
                break;
            }
            self.at += 1;
        }
        let text = &self.source[start..self.at];
        // LuaJIT's literals may group digits with underscores (`1_000`); `tonumber` does not take them.
        let digits: Vec<u8> = text.iter().copied().filter(|&c| c != b'_').collect();
        number::parse(&digits)
            .map(Token::Number)
            .ok_or_else(|| self.error(format!("malformed number near '{}'", text.escape_ascii())))
    }
}

/// Appends a code point as UTF-8, up to the 6-byte forms LuaJIT accepts.
fn utf8(code: u32, out: &mut Vec<u8>) {
    let (length, lead) = match code {
        0..0x80 => {
            out.push(code as u8);
            return;
        }
        0x80..0x800 => (2, 0xc0),
        0x800..0x1_0000 => (3, 0xe0),
        0x1_0000..0x20_0000 => (4, 0xf0),
        0x20_0000..0x400_0000 => (5, 0xf8),
        _ => (6, 0xfc),
    };
    out.push(lead | (code >> (6 * (length - 1))) as u8);
    for i in (0..length - 1).rev() {
        out.push(0x80 | ((code >> (6 * i)) & 0x3f) as u8);
    }
}
