//! Tokens to the syntax tree: Lua 5.1's grammar with LuaJIT's `goto` and labels.
//!
//! The parser recurses on nesting (blocks, parentheses, constructors, functions, right-associative operators), so the
//! nesting is bounded by `Limits::syntax_depth`; left-associative chains (`a + b + c ...`) are loops and nest nothing.
//! Every node is counted against a byte budget, so a source cannot build an unbounded tree.

use crate::ast::{BinOp, Block, Expr, ExprKind, Field, FunctionBody, Name, Statement, UnOp};
use crate::fuel::Fuel;
use crate::lexer::{Lexer, SyntaxError, Token};
use crate::{Limit, LuaError};

pub(crate) enum ParseError {
    Syntax(SyntaxError),
    Limit(Limit),
}

impl From<SyntaxError> for ParseError {
    fn from(e: SyntaxError) -> Self {
        ParseError::Syntax(e)
    }
}

type Parse<T> = Result<T, ParseError>;

pub(crate) struct Parser<'a, 'f> {
    lexer: Lexer<'a>,
    /// The fuel of the call that compiles, which each token read spends.
    fuel: &'f mut Fuel,
    token: Token,
    /// The line of `token`.
    line: u32,
    /// The line of the token before `token`.
    last_line: u32,
    /// The source text of `token`, which syntax errors show.
    text: &'a [u8],
    ahead: Option<(Token, u32, &'a [u8])>,
    depth: u32,
    max_depth: u32,
    /// Bytes of tree the parser may still build.
    budget: usize,
}

/// The bytes a node is counted as.
const NODE: usize = 64;

/// The fuel each token costs, spent as the lexer reads it: lexing, parsing and compiling take some 60 to 260 ns a token
/// whatever its length (the dearest are new names and strings, which the heap interns, and chains of fields and
/// operators), where the quarter fuel a byte that a source pays besides covers only reading its bytes. Eight keeps the
/// dearest source in `examples/compile_price.rs` within the ~35 ns a fuel the audit allows.
pub(crate) const TOKEN_FUEL: u64 = 8;

/// Parses a chunk.
pub(crate) fn parse(
    source: &[u8],
    max_depth: u32,
    max_string: usize,
    budget: usize,
    fuel: &mut Fuel,
) -> Parse<Block> {
    let mut lexer = Lexer::new(source, max_string);
    let token = read_token(&mut lexer, fuel)?;
    let line = lexer.line;
    let text = lexer.text();
    let mut parser = Parser {
        lexer,
        fuel,
        token,
        line,
        last_line: line,
        text,
        ahead: None,
        depth: 0,
        max_depth,
        budget,
    };
    let block = parser.block()?;
    if parser.token != Token::Eof {
        return Err(parser.error_near("'<eof>' expected"));
    }
    Ok(block)
}

/// The next token, its price paid before the lexer reads it: a source that the call cannot pay for stops being read
/// at the token that cannot be paid.
fn read_token(lexer: &mut Lexer<'_>, fuel: &mut Fuel) -> Parse<Token> {
    fuel.spend(TOKEN_FUEL).map_err(|error| match error {
        LuaError::Limit(limit) => ParseError::Limit(limit),
        LuaError::Runtime(_) => unreachable!("spending fuel only fails by running out"),
    })?;
    Ok(lexer.next_token()?)
}

fn binary_op(token: &Token) -> Option<(BinOp, u8, u8)> {
    Some(match token {
        Token::Or => (BinOp::Or, 1, 1),
        Token::And => (BinOp::And, 2, 2),
        Token::Less => (BinOp::Lt, 3, 3),
        Token::Greater => (BinOp::Gt, 3, 3),
        Token::LessEqual => (BinOp::Le, 3, 3),
        Token::GreaterEqual => (BinOp::Ge, 3, 3),
        Token::NotEqual => (BinOp::Ne, 3, 3),
        Token::Equal => (BinOp::Eq, 3, 3),
        Token::Concat => (BinOp::Concat, 5, 4),
        Token::Plus => (BinOp::Add, 6, 6),
        Token::Minus => (BinOp::Sub, 6, 6),
        Token::Star => (BinOp::Mul, 7, 7),
        Token::Slash => (BinOp::Div, 7, 7),
        Token::Percent => (BinOp::Mod, 7, 7),
        Token::Caret => (BinOp::Pow, 10, 9),
        _ => return None,
    })
}

const UNARY_PRIORITY: u8 = 8;

fn describe(token: &Token) -> String {
    match token {
        Token::Name(n) | Token::String(n) => crate::shown(n),
        Token::Number(n) => crate::number::format(*n),
        Token::Eof => "<eof>".into(),
        other => {
            let text = match other {
                Token::And => "and",
                Token::Break => "break",
                Token::Do => "do",
                Token::Else => "else",
                Token::Elseif => "elseif",
                Token::End => "end",
                Token::False => "false",
                Token::For => "for",
                Token::Function => "function",
                Token::If => "if",
                Token::In => "in",
                Token::Local => "local",
                Token::Nil => "nil",
                Token::Not => "not",
                Token::Or => "or",
                Token::Repeat => "repeat",
                Token::Return => "return",
                Token::Then => "then",
                Token::True => "true",
                Token::Until => "until",
                Token::While => "while",
                Token::Plus => "+",
                Token::Minus => "-",
                Token::Star => "*",
                Token::Slash => "/",
                Token::Percent => "%",
                Token::Caret => "^",
                Token::Hash => "#",
                Token::Equal => "==",
                Token::NotEqual => "~=",
                Token::LessEqual => "<=",
                Token::GreaterEqual => ">=",
                Token::Less => "<",
                Token::Greater => ">",
                Token::Assign => "=",
                Token::LeftParen => "(",
                Token::RightParen => ")",
                Token::LeftBrace => "{",
                Token::RightBrace => "}",
                Token::LeftBracket => "[",
                Token::RightBracket => "]",
                Token::DoubleColon => "::",
                Token::Semicolon => ";",
                Token::Colon => ":",
                Token::Comma => ",",
                Token::Dot => ".",
                Token::Concat => "..",
                Token::Dots => "...",
                _ => "?",
            };
            text.into()
        }
    }
}

impl<'a> Parser<'a, '_> {
    fn advance(&mut self) -> Parse<()> {
        self.last_line = self.line;
        match self.ahead.take() {
            Some((token, line, text)) => {
                self.token = token;
                self.line = line;
                self.text = text;
            }
            None => {
                self.token = read_token(&mut self.lexer, self.fuel)?;
                self.line = self.lexer.line;
                self.text = self.lexer.text();
            }
        }
        Ok(())
    }

    fn peek(&mut self) -> Parse<&Token> {
        if self.ahead.is_none() {
            let token = read_token(&mut self.lexer, self.fuel)?;
            self.ahead = Some((token, self.lexer.line, self.lexer.text()));
        }
        Ok(&self.ahead.as_ref().expect("a token ahead").0)
    }

    fn error_near(&self, message: &str) -> ParseError {
        ParseError::Syntax(SyntaxError {
            line: self.line,
            message: format!("{message} near '{}'", self.near()),
        })
    }

    /// The current token as LuaJIT's messages show it: a name or a number as written, a quoted string within its
    /// quotes with its escapes decoded, a long string as written.
    fn near(&self) -> String {
        match &self.token {
            Token::Name(_) | Token::Number(_) => crate::shown(self.text),
            Token::String(s) => match self.text.first() {
                Some(&quote @ (b'"' | b'\'')) => {
                    let mut shown = vec![quote];
                    shown.extend_from_slice(&s[..s.len().min(64)]);
                    shown.push(quote);
                    crate::shown(&shown)
                }
                _ => crate::shown(self.text),
            },
            other => describe(other),
        }
    }

    fn expect(&mut self, token: Token, what: &str) -> Parse<()> {
        if self.token != token {
            return Err(self.error_near(&format!("'{what}' expected")));
        }
        self.advance()
    }

    /// Expects the closing token of a construct opened at `line`.
    fn expect_match(&mut self, token: Token, what: &str, opener: &str, line: u32) -> Parse<()> {
        if self.token == token {
            return self.advance();
        }
        if line == self.line {
            Err(self.error_near(&format!("'{what}' expected")))
        } else {
            Err(self.error_near(&format!(
                "'{what}' expected (to close '{opener}' at line {line})"
            )))
        }
    }

    fn name(&mut self) -> Parse<Name> {
        match std::mem::replace(&mut self.token, Token::Eof) {
            Token::Name(n) => {
                self.charge(n.len())?;
                self.advance()?;
                Ok(n)
            }
            other => {
                self.token = other;
                Err(self.error_near("'<name>' expected"))
            }
        }
    }

    fn charge(&mut self, bytes: usize) -> Parse<()> {
        self.budget = self
            .budget
            .checked_sub(bytes + NODE)
            .ok_or(ParseError::Limit(Limit::Memory))?;
        Ok(())
    }

    fn enter(&mut self) -> Parse<()> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(ParseError::Syntax(SyntaxError {
                line: self.depth_line(),
                message: "chunk has too many syntax levels".into(),
            }));
        }
        Ok(())
    }

    fn depth_line(&self) -> u32 {
        self.line
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn block_ends(&self) -> bool {
        matches!(
            self.token,
            Token::Eof | Token::End | Token::Else | Token::Elseif | Token::Until
        )
    }

    fn block(&mut self) -> Parse<Block> {
        self.enter()?;
        let mut block = Block::default();
        loop {
            while self.token == Token::Semicolon {
                self.advance()?;
            }
            if self.block_ends() {
                break;
            }
            if self.token == Token::Return {
                let line = self.line;
                self.advance()?;
                let values = if self.block_ends() || self.token == Token::Semicolon {
                    Vec::new()
                } else {
                    self.expr_list()?
                };
                while self.token == Token::Semicolon {
                    self.advance()?;
                }
                // What closes the block comes next: whoever opened it says what was expected otherwise.
                block.ret = Some((values, line));
                break;
            }
            let statement = self.statement()?;
            let last = matches!(statement, Statement::Break { .. });
            block.statements.push(statement);
            // In Lua 5.1, as LuaJIT keeps it, `break` ends its block as `return` does.
            if last {
                while self.token == Token::Semicolon {
                    self.advance()?;
                }
                break;
            }
        }
        self.leave();
        Ok(block)
    }

    fn statement(&mut self) -> Parse<Statement> {
        self.charge(0)?;
        let line = self.line;
        // `goto` is a keyword only before a name, as in LuaJIT: `goto = 1` assigns a variable.
        let is_goto = matches!(&self.token, Token::Name(n) if **n == *b"goto")
            && matches!(self.peek()?, Token::Name(_));
        match self.token {
            Token::If => self.if_statement(line),
            Token::While => {
                self.advance()?;
                let condition = self.expr()?;
                self.expect(Token::Do, "do")?;
                let body = self.block()?;
                self.expect_match(Token::End, "end", "while", line)?;
                Ok(Statement::While { condition, body })
            }
            Token::Do => {
                self.advance()?;
                let body = self.block()?;
                self.expect_match(Token::End, "end", "do", line)?;
                Ok(Statement::Do(body))
            }
            Token::For => self.for_statement(line),
            Token::Repeat => {
                self.advance()?;
                let body = self.block()?;
                self.expect_match(Token::Until, "until", "repeat", line)?;
                let condition = self.expr()?;
                Ok(Statement::Repeat { body, condition })
            }
            Token::Function => {
                self.advance()?;
                let mut target = Expr {
                    kind: ExprKind::Name(self.name()?),
                    line,
                };
                let mut method = false;
                while !method && matches!(self.token, Token::Dot | Token::Colon) {
                    method = self.token == Token::Colon;
                    self.advance()?;
                    let key_line = self.line;
                    let key = self.name()?;
                    let key = Expr {
                        kind: ExprKind::String(key),
                        line: key_line,
                    };
                    target = Expr {
                        kind: ExprKind::Index(Box::new(target), Box::new(key)),
                        line,
                    };
                }
                let body = self.function_body(method, line)?;
                let value = Expr {
                    kind: ExprKind::Function(Box::new(body)),
                    line,
                };
                Ok(Statement::Assign {
                    targets: vec![target],
                    values: vec![value],
                    line,
                })
            }
            Token::Local => {
                self.advance()?;
                if self.token == Token::Function {
                    self.advance()?;
                    let name = self.name()?;
                    let body = self.function_body(false, line)?;
                    return Ok(Statement::LocalFunction {
                        name,
                        body: Box::new(body),
                    });
                }
                let mut names = vec![self.name()?];
                while self.token == Token::Comma {
                    self.advance()?;
                    names.push(self.name()?);
                }
                let values = if self.token == Token::Assign {
                    self.advance()?;
                    self.expr_list()?
                } else {
                    Vec::new()
                };
                Ok(Statement::Local {
                    names,
                    values,
                    line,
                })
            }
            Token::DoubleColon => {
                self.advance()?;
                let name = self.name()?;
                self.expect(Token::DoubleColon, "::")?;
                Ok(Statement::Label { name, line })
            }
            Token::Break => {
                self.advance()?;
                Ok(Statement::Break { line })
            }
            Token::Name(_) if is_goto => {
                self.advance()?;
                let label = self.name()?;
                Ok(Statement::Goto { label, line })
            }
            _ => self.expression_statement(line),
        }
    }

    fn if_statement(&mut self, line: u32) -> Parse<Statement> {
        let mut arms = Vec::new();
        let mut otherwise = None;
        self.advance()?;
        let condition = self.expr()?;
        self.expect(Token::Then, "then")?;
        arms.push((condition, self.block()?));
        loop {
            match self.token {
                Token::Elseif => {
                    self.advance()?;
                    let condition = self.expr()?;
                    self.expect(Token::Then, "then")?;
                    arms.push((condition, self.block()?));
                }
                Token::Else => {
                    self.advance()?;
                    otherwise = Some(self.block()?);
                    self.expect_match(Token::End, "end", "if", line)?;
                    break;
                }
                _ => {
                    self.expect_match(Token::End, "end", "if", line)?;
                    break;
                }
            }
        }
        Ok(Statement::If { arms, otherwise })
    }

    fn for_statement(&mut self, line: u32) -> Parse<Statement> {
        self.advance()?;
        let first = self.name()?;
        match self.token {
            Token::Assign => {
                self.advance()?;
                let start = self.expr()?;
                self.expect(Token::Comma, ",")?;
                let limit = self.expr()?;
                let step = if self.token == Token::Comma {
                    self.advance()?;
                    Some(self.expr()?)
                } else {
                    None
                };
                self.expect(Token::Do, "do")?;
                let body = self.block()?;
                self.expect_match(Token::End, "end", "for", line)?;
                Ok(Statement::NumericFor {
                    var: first,
                    start,
                    limit,
                    step,
                    body,
                    line,
                })
            }
            Token::Comma | Token::In => {
                let mut names = vec![first];
                while self.token == Token::Comma {
                    self.advance()?;
                    names.push(self.name()?);
                }
                self.expect(Token::In, "in")?;
                let call_line = self.line;
                let values = self.expr_list()?;
                self.expect(Token::Do, "do")?;
                let body = self.block()?;
                self.expect_match(Token::End, "end", "for", line)?;
                Ok(Statement::GenericFor {
                    names,
                    values,
                    body,
                    line,
                    call_line,
                })
            }
            _ => Err(self.error_near("'=' or 'in' expected")),
        }
    }

    /// A call, or an assignment: as in LuaJIT, each target must be a variable when the token after it is read, and
    /// a variable not followed by `,` wants `=`.
    fn expression_statement(&mut self, line: u32) -> Parse<Statement> {
        let first = self.suffixed_expr()?;
        if matches!(first.kind, ExprKind::Call(..) | ExprKind::Method(..)) {
            return Ok(Statement::Call(first));
        }
        let mut targets = vec![first];
        loop {
            let target = targets.last().expect("a target");
            if !matches!(target.kind, ExprKind::Name(_) | ExprKind::Index(..)) {
                return Err(self.error_near("syntax error"));
            }
            if self.token != Token::Comma {
                break;
            }
            self.advance()?;
            targets.push(self.suffixed_expr()?);
        }
        self.expect(Token::Assign, "=")?;
        let values = self.expr_list()?;
        Ok(Statement::Assign {
            targets,
            values,
            line,
        })
    }

    fn function_body(&mut self, method: bool, line: u32) -> Parse<FunctionBody> {
        self.enter()?;
        let mut params: Vec<Name> = Vec::new();
        if method {
            params.push(b"self".as_slice().into());
        }
        let mut vararg = false;
        self.expect(Token::LeftParen, "(")?;
        if self.token != Token::RightParen {
            loop {
                match self.token {
                    Token::Dots => {
                        self.advance()?;
                        vararg = true;
                        break;
                    }
                    Token::Name(_) => params.push(self.name()?),
                    _ => return Err(self.error_near("'<name>' expected")),
                }
                if self.token != Token::Comma {
                    break;
                }
                self.advance()?;
            }
        }
        self.expect(Token::RightParen, ")")?;
        let body = self.block()?;
        let end_line = self.line;
        self.expect_match(Token::End, "end", "function", line)?;
        self.leave();
        Ok(FunctionBody {
            params,
            vararg,
            body,
            line,
            end_line,
        })
    }

    fn expr_list(&mut self) -> Parse<Vec<Expr>> {
        let mut list = vec![self.expr()?];
        while self.token == Token::Comma {
            self.advance()?;
            list.push(self.expr()?);
        }
        Ok(list)
    }

    pub(crate) fn expr(&mut self) -> Parse<Expr> {
        self.subexpr(0)
    }

    fn subexpr(&mut self, limit: u8) -> Parse<Expr> {
        self.enter()?;
        self.charge(0)?;
        let line = self.line;
        let unary = match self.token {
            Token::Not => Some(UnOp::Not),
            Token::Minus => Some(UnOp::Neg),
            Token::Hash => Some(UnOp::Len),
            _ => None,
        };
        let mut left = if let Some(op) = unary {
            self.advance()?;
            let operand = self.subexpr(UNARY_PRIORITY)?;
            Expr {
                kind: ExprKind::Unary(op, Box::new(operand)),
                line,
            }
        } else {
            self.simple_expr()?
        };
        while let Some((op, left_priority, right_priority)) = binary_op(&self.token) {
            if left_priority <= limit {
                break;
            }
            let op_line = self.line;
            self.advance()?;
            self.charge(0)?;
            let right = self.subexpr(right_priority)?;
            left = Expr {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                line: op_line,
            };
        }
        self.leave();
        Ok(left)
    }

    fn simple_expr(&mut self) -> Parse<Expr> {
        let line = self.line;
        let kind = match &self.token {
            Token::Number(n) => ExprKind::Number(*n),
            Token::String(_) => {
                let Token::String(s) = std::mem::replace(&mut self.token, Token::Eof) else {
                    unreachable!()
                };
                self.charge(s.len())?;
                ExprKind::String(s)
            }
            Token::Nil => ExprKind::Nil,
            Token::True => ExprKind::True,
            Token::False => ExprKind::False,
            Token::Dots => ExprKind::Dots,
            Token::LeftBrace => return self.table(),
            Token::Function => {
                self.advance()?;
                let body = self.function_body(false, line)?;
                return Ok(Expr {
                    kind: ExprKind::Function(Box::new(body)),
                    line,
                });
            }
            _ => return self.suffixed_expr(),
        };
        self.advance()?;
        Ok(Expr { kind, line })
    }

    fn primary_expr(&mut self) -> Parse<Expr> {
        let line = self.line;
        match self.token {
            Token::Name(_) => Ok(Expr {
                kind: ExprKind::Name(self.name()?),
                line,
            }),
            Token::LeftParen => {
                self.advance()?;
                let inner = self.expr()?;
                self.expect_match(Token::RightParen, ")", "(", line)?;
                Ok(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    line,
                })
            }
            _ => Err(self.error_near("unexpected symbol")),
        }
    }

    fn suffixed_expr(&mut self) -> Parse<Expr> {
        let mut expr = self.primary_expr()?;
        loop {
            self.charge(0)?;
            let line = self.line;
            match self.token {
                Token::Dot => {
                    self.advance()?;
                    let key_line = self.line;
                    let key = Expr {
                        kind: ExprKind::String(self.name()?),
                        line: key_line,
                    };
                    expr = Expr {
                        kind: ExprKind::Index(Box::new(expr), Box::new(key)),
                        line,
                    };
                }
                Token::LeftBracket => {
                    self.advance()?;
                    let key = self.expr()?;
                    self.expect(Token::RightBracket, "]")?;
                    expr = Expr {
                        kind: ExprKind::Index(Box::new(expr), Box::new(key)),
                        line,
                    };
                }
                Token::Colon => {
                    self.advance()?;
                    let name = self.name()?;
                    let args = self.call_args()?;
                    expr = Expr {
                        kind: ExprKind::Method(Box::new(expr), name, args),
                        line,
                    };
                }
                Token::LeftParen | Token::String(_) | Token::LeftBrace => {
                    let args = self.call_args()?;
                    expr = Expr {
                        kind: ExprKind::Call(Box::new(expr), args),
                        line,
                    };
                }
                _ => return Ok(expr),
            }
        }
    }

    fn call_args(&mut self) -> Parse<Vec<Expr>> {
        let line = self.line;
        match self.token {
            Token::String(_) => {
                let Token::String(s) = std::mem::replace(&mut self.token, Token::Eof) else {
                    unreachable!()
                };
                self.charge(s.len())?;
                self.advance()?;
                Ok(vec![Expr {
                    kind: ExprKind::String(s),
                    line,
                }])
            }
            Token::LeftBrace => Ok(vec![self.table()?]),
            Token::LeftParen => {
                if line != self.last_line {
                    return Err(self.error_near("ambiguous syntax (function call x new statement)"));
                }
                self.advance()?;
                let args = if self.token == Token::RightParen {
                    Vec::new()
                } else {
                    self.expr_list()?
                };
                self.expect_match(Token::RightParen, ")", "(", line)?;
                Ok(args)
            }
            _ => Err(self.error_near("function arguments expected")),
        }
    }

    fn table(&mut self) -> Parse<Expr> {
        self.enter()?;
        let line = self.line;
        self.expect(Token::LeftBrace, "{")?;
        let mut fields = Vec::new();
        while self.token != Token::RightBrace {
            self.charge(0)?;
            let keyed_name = matches!(self.token, Token::Name(_)) && *self.peek()? == Token::Assign;
            let field = match self.token {
                Token::LeftBracket => {
                    self.advance()?;
                    let key = self.expr()?;
                    self.expect(Token::RightBracket, "]")?;
                    self.expect(Token::Assign, "=")?;
                    Field::Keyed(key, self.expr()?)
                }
                Token::Name(_) if keyed_name => {
                    let key_line = self.line;
                    let key = Expr {
                        kind: ExprKind::String(self.name()?),
                        line: key_line,
                    };
                    self.advance()?;
                    Field::Keyed(key, self.expr()?)
                }
                _ => Field::Positional(self.expr()?),
            };
            fields.push(field);
            match self.token {
                Token::Comma | Token::Semicolon => self.advance()?,
                _ => break,
            }
        }
        self.expect_match(Token::RightBrace, "}", "{", line)?;
        self.leave();
        Ok(Expr {
            kind: ExprKind::Table(fields),
            line,
        })
    }
}
