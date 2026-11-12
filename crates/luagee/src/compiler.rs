//! The syntax tree to register bytecode.
//!
//! A local that a nested function captures lives in a heap cell from its declaration on (a resolver pass finds
//! them first), so a closure shares the variable itself, each loop iteration gets a fresh one, and nothing needs
//! closing when a scope ends. Long left-associative chains (`a + b + c ...`, `x or y or z ...`) compile in loops;
//! other nesting is bounded by `Limits::syntax_depth`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::Limit;
use crate::ast::{BinOp, Block, Expr, ExprKind, Field, FunctionBody, Name, Statement, UnOp};
use crate::heap::Heap;
use crate::lexer::SyntaxError;
use crate::parser::ParseError;
use crate::proto::{ArithOp, Capture, CompareOp, LocalInfo, Op, Proto, Rk};
use crate::value::{Handle, StrId, Value};

type Compile<T> = Result<T, ParseError>;

/// The registers a function may use (Lua 5.1's limit is 250).
const MAX_REGISTERS: u32 = 250;
/// The locals a function may have active at once (LuaJIT's and Lua 5.1's limit).
const MAX_LOCALS: usize = 200;
/// The upvalues a function may have (LuaJIT's limit).
const MAX_UPVALUES: usize = 60;
/// Values a table constructor stores per `SetList`.
const FIELDS_PER_FLUSH: u32 = 50;

/// Compiles a chunk: a vararg function of no parameters.
pub(crate) fn compile(
    block: &Block,
    chunk: Rc<str>,
    source: StrId,
    heap: &mut Heap,
    max_depth: u32,
) -> Compile<Rc<Proto>> {
    let mut resolver = Resolver {
        functions: Vec::new(),
        captured: Vec::new(),
        depth: 0,
        max_depth,
    };
    resolver.function(&[], block)?;
    let mut compiler = Compiler {
        heap,
        functions: Vec::new(),
        captured: resolver.captured,
        declared: 0,
        chunk,
        source,
        depth: 0,
        max_depth,
    };
    compiler.function_proto(&[], true, block, 0, 0)
}

// ---------------------------------------------------------------------------------------------------------------
// The resolver: which local declarations a nested function captures. It declares names in exactly the order the
// compiler does, so the n-th declaration of one is the n-th of the other.

struct Resolver {
    /// Per function being resolved, its blocks of (name, declaration) pairs.
    functions: Vec<Vec<Vec<(Name, usize)>>>,
    captured: Vec<bool>,
    depth: u32,
    max_depth: u32,
}

impl Resolver {
    fn depth_line(&self) -> u32 {
        0
    }

    fn enter(&mut self) -> Compile<()> {
        self.depth += 1;
        if self.depth > self.max_depth.saturating_mul(2) {
            return Err(ParseError::Syntax(SyntaxError {
                line: self.depth_line(),
                message: "chunk has too many syntax levels".into(),
            }));
        }
        Ok(())
    }

    fn declare(&mut self, name: &Name) {
        let id = self.captured.len();
        self.captured.push(false);
        let blocks = self.functions.last_mut().expect("a function");
        blocks.last_mut().expect("a block").push((name.clone(), id));
    }

    fn reference(&mut self, name: &Name) {
        let current = self.functions.len() - 1;
        for f in (0..self.functions.len()).rev() {
            let found = self.functions[f].iter().rev().find_map(|block| {
                block
                    .iter()
                    .rev()
                    .find(|(n, _)| n == name)
                    .map(|(_, id)| *id)
            });
            if let Some(id) = found {
                if f != current {
                    self.captured[id] = true;
                }
                return;
            }
        }
    }

    fn function(&mut self, params: &[Name], body: &Block) -> Compile<()> {
        self.enter()?;
        self.functions.push(vec![Vec::new()]);
        for param in params {
            self.declare(param);
        }
        self.block_contents(body)?;
        self.functions.pop();
        self.depth -= 1;
        Ok(())
    }

    fn block(&mut self, block: &Block) -> Compile<()> {
        self.functions
            .last_mut()
            .expect("a function")
            .push(Vec::new());
        self.block_contents(block)?;
        self.functions.last_mut().expect("a function").pop();
        Ok(())
    }

    fn block_contents(&mut self, block: &Block) -> Compile<()> {
        self.enter()?;
        for statement in &block.statements {
            self.statement(statement)?;
        }
        if let Some((values, _)) = &block.ret {
            for value in values {
                self.expr(value)?;
            }
        }
        self.depth -= 1;
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Compile<()> {
        match statement {
            Statement::Call(e) => self.expr(e),
            Statement::Local { names, values, .. } => {
                for value in values {
                    self.expr(value)?;
                }
                for name in names {
                    self.declare(name);
                }
                Ok(())
            }
            Statement::Assign {
                targets, values, ..
            } => {
                for target in targets {
                    self.expr(target)?;
                }
                for value in values {
                    self.expr(value)?;
                }
                Ok(())
            }
            Statement::Do(block) => self.block(block),
            Statement::While { condition, body } => {
                self.expr(condition)?;
                self.block(body)
            }
            Statement::Repeat { body, condition } => {
                self.functions
                    .last_mut()
                    .expect("a function")
                    .push(Vec::new());
                for statement in &body.statements {
                    self.statement(statement)?;
                }
                if let Some((values, _)) = &body.ret {
                    for value in values {
                        self.expr(value)?;
                    }
                }
                self.expr(condition)?;
                self.functions.last_mut().expect("a function").pop();
                Ok(())
            }
            Statement::If { arms, otherwise } => {
                for (condition, block) in arms {
                    self.expr(condition)?;
                    self.block(block)?;
                }
                if let Some(block) = otherwise {
                    self.block(block)?;
                }
                Ok(())
            }
            Statement::NumericFor {
                var,
                start,
                limit,
                step,
                body,
                ..
            } => {
                self.expr(start)?;
                self.expr(limit)?;
                if let Some(step) = step {
                    self.expr(step)?;
                }
                self.functions
                    .last_mut()
                    .expect("a function")
                    .push(Vec::new());
                self.declare(var);
                self.block(body)?;
                self.functions.last_mut().expect("a function").pop();
                Ok(())
            }
            Statement::GenericFor {
                names,
                values,
                body,
                ..
            } => {
                for value in values {
                    self.expr(value)?;
                }
                self.functions
                    .last_mut()
                    .expect("a function")
                    .push(Vec::new());
                for name in names {
                    self.declare(name);
                }
                self.block(body)?;
                self.functions.last_mut().expect("a function").pop();
                Ok(())
            }
            Statement::LocalFunction { name, body } => {
                self.declare(name);
                self.function(&body.params, &body.body)
            }
            Statement::Break { .. } | Statement::Goto { .. } | Statement::Label { .. } => Ok(()),
        }
    }

    /// Walks an expression in the compiler's order, with an explicit stack so long chains do not recurse.
    fn expr(&mut self, expr: &Expr) -> Compile<()> {
        let mut pending: Vec<&Expr> = vec![expr];
        while let Some(e) = pending.pop() {
            match &e.kind {
                ExprKind::Name(name) => self.reference(name),
                ExprKind::Function(body) => self.function(&body.params, &body.body)?,
                ExprKind::Binary(_, left, right) | ExprKind::Index(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                ExprKind::Unary(_, inner) | ExprKind::Paren(inner) => pending.push(inner),
                ExprKind::Call(function, args) => {
                    pending.extend(args.iter().rev());
                    pending.push(function);
                }
                ExprKind::Method(object, _, args) => {
                    pending.extend(args.iter().rev());
                    pending.push(object);
                }
                ExprKind::Table(fields) => {
                    for field in fields.iter().rev() {
                        match field {
                            Field::Positional(value) => pending.push(value),
                            Field::Keyed(key, value) => {
                                pending.push(value);
                                pending.push(key);
                            }
                        }
                    }
                }
                ExprKind::Nil
                | ExprKind::True
                | ExprKind::False
                | ExprKind::Dots
                | ExprKind::Number(_)
                | ExprKind::String(_) => {}
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The compiler.

struct LocalVar {
    name: Name,
    register: u8,
    captured: bool,
    /// Its entry in the function's debug list.
    info: usize,
}

struct PendingGoto {
    jump: usize,
    active: usize,
    line: u32,
}

struct BlockState {
    /// Locals active when the block began.
    active: usize,
    /// The first free register when the block began.
    free: u32,
    is_loop: bool,
    is_repeat: bool,
    breaks: Vec<usize>,
    /// Labels by name: where each is. Maps, so that many labels and gotos compile in n log n.
    labels: BTreeMap<Name, usize>,
    /// Gotos not resolved yet, by the label they want.
    gotos: BTreeMap<Name, Vec<PendingGoto>>,
}

struct FunctionState {
    code: Vec<Op>,
    lines: Vec<u32>,
    constants: Vec<Value>,
    numbers: BTreeMap<u64, u32>,
    strings: BTreeMap<Handle, u32>,
    nil: Option<u32>,
    booleans: [Option<u32>; 2],
    protos: Vec<Rc<Proto>>,
    captures: Vec<Capture>,
    upvalue_names: Vec<Name>,
    locals: Vec<LocalVar>,
    infos: Vec<LocalInfo>,
    blocks: Vec<BlockState>,
    free: u32,
    registers: u32,
    /// The line being compiled.
    line: u32,
    /// The line the function begins on; 0 for the main chunk.
    defined: u32,
    vararg: bool,
}

enum Var {
    Local(u8, bool),
    Upvalue(u8),
    Global(u32),
}

/// How many values a multi-valued expression should give.
#[derive(Clone, Copy)]
enum Want {
    Fixed(u8),
    All,
}

impl Want {
    fn encode(self) -> u8 {
        match self {
            Want::Fixed(n) => n + 1,
            Want::All => 0,
        }
    }
}

struct Compiler<'h> {
    heap: &'h mut Heap,
    functions: Vec<FunctionState>,
    captured: Vec<bool>,
    declared: usize,
    chunk: Rc<str>,
    source: StrId,
    depth: u32,
    max_depth: u32,
}

fn is_arith(op: BinOp) -> Option<ArithOp> {
    Some(match op {
        BinOp::Add => ArithOp::Add,
        BinOp::Sub => ArithOp::Sub,
        BinOp::Mul => ArithOp::Mul,
        BinOp::Div => ArithOp::Div,
        BinOp::Mod => ArithOp::Mod,
        BinOp::Pow => ArithOp::Pow,
        _ => return None,
    })
}

impl Compiler<'_> {
    fn f(&mut self) -> &mut FunctionState {
        self.functions
            .last_mut()
            .expect("a function being compiled")
    }

    fn syntax(&self, line: u32, message: impl Into<String>) -> ParseError {
        ParseError::Syntax(SyntaxError {
            line,
            message: message.into(),
        })
    }

    fn enter(&mut self) -> Compile<()> {
        self.depth += 1;
        if self.depth > self.max_depth.saturating_mul(2) {
            return Err(ParseError::Syntax(SyntaxError {
                line: self.depth_line(),
                message: "chunk has too many syntax levels".into(),
            }));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn depth_line(&self) -> u32 {
        self.functions.last().map_or(0, |f| f.line)
    }

    fn emit(&mut self, op: Op) -> usize {
        let f = self.f();
        f.code.push(op);
        let line = f.line;
        f.lines.push(line);
        f.code.len() - 1
    }

    fn pc(&mut self) -> usize {
        self.f().code.len()
    }

    fn jump(&mut self) -> usize {
        self.emit(Op::Jump { offset: 0 })
    }

    /// Points the jump at `from` to `to`.
    fn patch(&mut self, from: usize, to: usize) {
        let offset = to as i64 - from as i64 - 1;
        let offset = i32::try_from(offset).expect("a function shorter than 2^31 instructions");
        match &mut self.f().code[from] {
            Op::Jump { offset: o }
            | Op::ForPrep { offset: o, .. }
            | Op::ForLoop { offset: o, .. }
            | Op::ForEachLoop { offset: o, .. } => *o = offset,
            other => unreachable!("patching {other:?}"),
        }
    }

    fn patch_here(&mut self, jumps: Vec<usize>) {
        let here = self.pc();
        for jump in jumps {
            self.patch(jump, here);
        }
    }

    fn alloc(&mut self, count: u32) -> Compile<u8> {
        let line = self.f().line;
        let f = self.f();
        let first = f.free;
        f.free += count;
        if f.free > MAX_REGISTERS {
            return Err(self.syntax(line, "function or expression too complex"));
        }
        f.registers = f.registers.max(f.free);
        Ok(first as u8)
    }

    fn free(&mut self) -> u32 {
        self.f().free
    }

    fn set_free(&mut self, free: u32) {
        self.f().free = free;
    }

    fn constant(&mut self, value: Value) -> Compile<u32> {
        let f = self.f();
        let existing = match value {
            Value::Number(n) => f.numbers.get(&n.to_bits()).copied(),
            Value::String(s) => f.strings.get(&s.0).copied(),
            Value::Nil => f.nil,
            Value::Boolean(b) => f.booleans[usize::from(b)],
            _ => unreachable!("a constant is a number, string, boolean or nil"),
        };
        if let Some(k) = existing {
            return Ok(k);
        }
        let k = u32::try_from(f.constants.len()).map_err(|_| ParseError::Limit(Limit::Memory))?;
        f.constants.push(value);
        match value {
            Value::Number(n) => {
                f.numbers.insert(n.to_bits(), k);
            }
            Value::String(s) => {
                f.strings.insert(s.0, k);
            }
            Value::Nil => f.nil = Some(k),
            Value::Boolean(b) => f.booleans[usize::from(b)] = Some(k),
            _ => {}
        }
        Ok(k)
    }

    fn string_constant(&mut self, bytes: &[u8]) -> Compile<u32> {
        let s = self
            .heap
            .intern(bytes)
            .map_err(|_| ParseError::Limit(Limit::Memory))?;
        self.constant(Value::String(s))
    }

    /// The constant an expression folds to, if it is one.
    fn constant_of(&mut self, e: &Expr) -> Compile<Option<Value>> {
        Ok(Some(match &e.kind {
            ExprKind::Nil => Value::Nil,
            ExprKind::True => Value::Boolean(true),
            ExprKind::False => Value::Boolean(false),
            ExprKind::Number(n) => Value::Number(*n),
            ExprKind::String(s) => Value::String(
                self.heap
                    .intern(s)
                    .map_err(|_| ParseError::Limit(Limit::Memory))?,
            ),
            ExprKind::Unary(UnOp::Neg, inner) => match inner.kind {
                ExprKind::Number(n) => Value::Number(crate::number::neg(n)),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        }))
    }

    // --- variables -------------------------------------------------------------------------------------------

    /// A function past one of LuaJIT's limits, as LuaJIT says it: on the line being compiled.
    fn limit(&self, function: usize, limit: usize, what: &str) -> ParseError {
        let line = self.functions.last().expect("a function").line;
        let message = match self.functions[function].defined {
            0 => format!("main function has more than {limit} {what}"),
            defined => format!("function at line {defined} has more than {limit} {what}"),
        };
        self.syntax(line, message)
    }

    fn declare_local(&mut self, name: Name, register: u8) -> Compile<bool> {
        let captured = self.captured[self.declared];
        self.declared += 1;
        if self.f().locals.len() >= MAX_LOCALS {
            return Err(self.limit(self.functions.len() - 1, MAX_LOCALS, "local variables"));
        }
        let f = self.f();
        let info = f.infos.len();
        f.infos.push(LocalInfo {
            name: name.clone(),
            register,
            start: f.code.len(),
            end: usize::MAX,
        });
        f.locals.push(LocalVar {
            name,
            register,
            captured,
            info,
        });
        if captured {
            self.emit(Op::NewCell { a: register });
        }
        Ok(captured)
    }

    fn resolve(&mut self, name: &Name) -> Compile<Var> {
        let current = self.functions.len() - 1;
        if let Some(local) = self.functions[current]
            .locals
            .iter()
            .rev()
            .find(|l| l.name == *name)
        {
            return Ok(Var::Local(local.register, local.captured));
        }
        match self.upvalue(current, name)? {
            Some(i) => Ok(Var::Upvalue(i)),
            None => Ok(Var::Global(self.string_constant(name)?)),
        }
    }

    fn upvalue(&mut self, function: usize, name: &Name) -> Compile<Option<u8>> {
        if function == 0 {
            return Ok(None);
        }
        if let Some(i) = self.functions[function]
            .upvalue_names
            .iter()
            .position(|n| n == name)
        {
            return Ok(Some(i as u8));
        }
        let parent = function - 1;
        let capture = if let Some(local) = self.functions[parent]
            .locals
            .iter()
            .rev()
            .find(|l| l.name == *name)
        {
            assert!(
                local.captured,
                "the resolver marks every local a nested function uses"
            );
            Capture::Register(local.register)
        } else if let Some(i) = self.upvalue(parent, name)? {
            Capture::Upvalue(i)
        } else {
            return Ok(None);
        };
        if self.functions[function].captures.len() >= MAX_UPVALUES {
            return Err(self.limit(function, MAX_UPVALUES, "upvalues"));
        }
        let f = &mut self.functions[function];
        f.captures.push(capture);
        f.upvalue_names.push(name.clone());
        Ok(Some((f.captures.len() - 1) as u8))
    }

    // --- functions and blocks --------------------------------------------------------------------------------

    fn function_proto(
        &mut self,
        params: &[Name],
        vararg: bool,
        body: &Block,
        line: u32,
        end_line: u32,
    ) -> Compile<Rc<Proto>> {
        self.enter()?;
        self.functions.push(FunctionState {
            code: Vec::new(),
            lines: Vec::new(),
            constants: Vec::new(),
            numbers: BTreeMap::new(),
            strings: BTreeMap::new(),
            nil: None,
            booleans: [None; 2],
            protos: Vec::new(),
            captures: Vec::new(),
            upvalue_names: Vec::new(),
            locals: Vec::new(),
            infos: Vec::new(),
            blocks: Vec::new(),
            free: 0,
            registers: 2,
            line,
            defined: line,
            vararg,
        });
        if params.len() > MAX_LOCALS {
            return Err(self.limit(self.functions.len() - 1, MAX_LOCALS, "local variables"));
        }
        let first = self.alloc(params.len() as u32)?;
        for (i, param) in params.iter().enumerate() {
            self.declare_local(param.clone(), first + i as u8)?;
        }
        self.block(body, false, false)?;
        self.f().line = end_line;
        self.emit(Op::Return { first: 0, count: 1 });
        let f = self.functions.pop().expect("the function being compiled");
        let proto = Proto {
            code: f.code,
            lines: f.lines,
            constants: f.constants,
            protos: f.protos,
            captures: f.captures,
            locals: f.infos,
            upvalue_names: f.upvalue_names,
            params: params.len() as u8,
            vararg,
            registers: f.registers as u8,
            chunk: self.chunk.clone(),
            source: self.source,
            line,
            visited: Cell::new(0),
        };
        self.heap
            .charge(proto.bytes())
            .map_err(|_| ParseError::Limit(Limit::Memory))?;
        self.leave();
        Ok(Rc::new(proto))
    }

    fn block(&mut self, block: &Block, is_loop: bool, is_repeat: bool) -> Compile<()> {
        self.begin_block(is_loop, is_repeat);
        self.statements(block)?;
        self.end_block()
    }

    fn begin_block(&mut self, is_loop: bool, is_repeat: bool) {
        let f = self.f();
        let state = BlockState {
            active: f.locals.len(),
            free: f.free,
            is_loop,
            is_repeat,
            breaks: Vec::new(),
            labels: BTreeMap::new(),
            gotos: BTreeMap::new(),
        };
        f.blocks.push(state);
    }

    fn statements(&mut self, block: &Block) -> Compile<()> {
        self.enter()?;
        for (i, statement) in block.statements.iter().enumerate() {
            let at_end = block.ret.is_none()
                && block.statements[i + 1..]
                    .iter()
                    .all(|s| matches!(s, Statement::Label { .. }));
            self.statement(statement, at_end)?;
            let block_free = self.f().blocks.last().expect("a block").free;
            let free = self
                .f()
                .locals
                .last()
                .map_or(block_free, |l| u32::from(l.register) + 1);
            self.set_free(free.max(block_free));
        }
        if let Some((values, line)) = &block.ret {
            self.f().line = *line;
            self.ret(values)?;
        }
        self.leave();
        Ok(())
    }

    fn end_block(&mut self) -> Compile<()> {
        let block = self.f().blocks.pop().expect("a block");
        let f = self.f();
        let end = f.code.len();
        for local in &f.locals[block.active..] {
            let info = &mut f.infos[local.info];
            info.end = info.end.min(end);
        }
        f.locals.truncate(block.active);
        f.free = block.free;
        let breaks = block.breaks;
        if block.is_loop {
            self.patch_here(breaks);
        } else if !breaks.is_empty() {
            // A break inside a non-loop block belongs to the loop around it.
            let outer = self
                .f()
                .blocks
                .iter_mut()
                .rev()
                .find(|b| b.is_loop)
                .expect("break was checked to be in a loop");
            outer.breaks.extend(breaks);
        }
        let gotos = block.gotos;
        if gotos.is_empty() {
            return Ok(());
        }
        match self.f().blocks.last_mut() {
            Some(parent) => {
                let mut backward = Vec::new();
                for (name, list) in gotos {
                    // A label the parent already has is the one these gotos jump back to: the parent can have no
                    // other of that name.
                    if let Some(&pc) = parent.labels.get(&name) {
                        backward.extend(list.into_iter().map(|goto| (goto.jump, pc)));
                        continue;
                    }
                    let moved = list.into_iter().map(|goto| PendingGoto {
                        active: goto.active.min(block.active),
                        ..goto
                    });
                    parent.gotos.entry(name).or_default().extend(moved);
                }
                for (jump, pc) in backward {
                    self.patch(jump, pc);
                }
                Ok(())
            }
            None => {
                // The first unresolved goto in the source is the one reported.
                let (name, goto) = gotos
                    .iter()
                    .flat_map(|(name, list)| list.iter().map(move |goto| (name, goto)))
                    .min_by_key(|(_, goto)| goto.jump)
                    .expect("a pending goto");
                Err(self.syntax(
                    goto.line,
                    format!("undefined label '{}'", crate::shown(name)),
                ))
            }
        }
    }

    // --- statements ------------------------------------------------------------------------------------------

    fn statement(&mut self, statement: &Statement, at_end: bool) -> Compile<()> {
        match statement {
            Statement::Call(e) => {
                self.f().line = e.line;
                let base = self.free();
                self.call(e, Want::Fixed(0))?;
                self.set_free(base);
                Ok(())
            }
            Statement::Local {
                names,
                values,
                line,
            } => {
                self.f().line = *line;
                let base = self.alloc(0)?;
                self.expr_list(values, names.len())?;
                for (i, name) in names.iter().enumerate() {
                    self.declare_local(name.clone(), base + i as u8)?;
                }
                self.set_free(u32::from(base) + names.len() as u32);
                Ok(())
            }
            Statement::Assign {
                targets,
                values,
                line,
            } => {
                self.f().line = *line;
                self.assign(targets, values)
            }
            Statement::Do(block) => self.block(block, false, false),
            Statement::While { condition, body } => {
                let start = self.pc();
                let exits = self.jump_if(condition, false)?;
                self.begin_block(true, false);
                self.statements(body)?;
                let back = self.jump();
                self.patch(back, start);
                self.patch_here(exits);
                self.end_block()
            }
            Statement::Repeat { body, condition } => {
                let start = self.pc();
                self.begin_block(true, true);
                self.begin_block(false, true);
                self.statements(body)?;
                let again = self.jump_if(condition, false)?;
                for jump in again {
                    self.patch(jump, start);
                }
                self.end_block()?;
                self.end_block()
            }
            Statement::If { arms, otherwise } => {
                let mut escapes = Vec::new();
                for (i, (condition, block)) in arms.iter().enumerate() {
                    let skip = self.jump_if(condition, false)?;
                    self.block(block, false, false)?;
                    if i + 1 < arms.len() || otherwise.is_some() {
                        escapes.push(self.jump());
                    }
                    self.patch_here(skip);
                }
                if let Some(block) = otherwise {
                    self.block(block, false, false)?;
                }
                self.patch_here(escapes);
                Ok(())
            }
            Statement::NumericFor {
                var,
                start,
                limit,
                step,
                body,
                line,
            } => self.numeric_for(var, start, limit, step.as_ref(), body, *line),
            Statement::GenericFor {
                names,
                values,
                body,
                line,
                call_line,
            } => self.generic_for(names, values, body, (*line, *call_line)),
            Statement::LocalFunction { name, body } => {
                self.f().line = body.line;
                let register = self.alloc(1)?;
                let captured = self.declare_local(name.clone(), register)?;
                if captured {
                    let temp = self.alloc(1)?;
                    self.closure(body, temp)?;
                    self.emit(Op::SetCell {
                        cell: register,
                        b: temp,
                    });
                } else {
                    self.closure(body, register)?;
                }
                Ok(())
            }
            Statement::Break { line } => {
                self.f().line = *line;
                if !self.f().blocks.iter().any(|b| b.is_loop) {
                    return Err(self.syntax(*line, "no loop to break near 'break'"));
                }
                let jump = self.jump();
                self.f()
                    .blocks
                    .last_mut()
                    .expect("a block")
                    .breaks
                    .push(jump);
                Ok(())
            }
            Statement::Goto { label, line } => self.goto(label, *line),
            Statement::Label { name, line } => self.label(name, *line, at_end),
        }
    }

    /// A goto jumps to the label of the innermost block that has one of its name, as LuaJIT resolves them: a label of
    /// its own block seen already, else one later in its block or, when that block ends, in the enclosing ones.
    fn goto(&mut self, name: &Name, line: u32) -> Compile<()> {
        self.f().line = line;
        let jump = self.jump();
        let target = self
            .f()
            .blocks
            .last()
            .expect("a block")
            .labels
            .get(name)
            .copied();
        if let Some(pc) = target {
            self.patch(jump, pc);
            return Ok(());
        }
        let active = self.f().locals.len();
        self.f()
            .blocks
            .last_mut()
            .expect("a block")
            .gotos
            .entry(name.clone())
            .or_default()
            .push(PendingGoto { jump, active, line });
        Ok(())
    }

    fn label(&mut self, name: &Name, line: u32, at_end: bool) -> Compile<()> {
        if self
            .f()
            .blocks
            .last()
            .expect("a block")
            .labels
            .contains_key(name)
        {
            return Err(self.syntax(line, format!("duplicate label '{}'", crate::shown(name))));
        }
        let pc = self.pc();
        let block = self.f().blocks.last().expect("a block");
        // A label at the end of its block is outside the block's locals (not in a repeat, whose test sees them).
        let active = if at_end && !block.is_repeat {
            block.active
        } else {
            self.f().locals.len()
        };
        let pending = self
            .f()
            .blocks
            .last_mut()
            .expect("a block")
            .gotos
            .remove(name)
            .unwrap_or_default();
        for goto in pending {
            if goto.active < active {
                let local = &self.f().locals[goto.active].name;
                let message = format!(
                    "<goto {}> jumps into the scope of local '{}'",
                    crate::shown(name),
                    crate::shown(local)
                );
                return Err(self.syntax(goto.line, message));
            }
            self.patch(goto.jump, pc);
        }
        self.f()
            .blocks
            .last_mut()
            .expect("a block")
            .labels
            .insert(name.clone(), pc);
        Ok(())
    }

    fn numeric_for(
        &mut self,
        var: &Name,
        start: &Expr,
        limit: &Expr,
        step: Option<&Expr>,
        body: &Block,
        line: u32,
    ) -> Compile<()> {
        self.f().line = line;
        let base = self.alloc(0)?;
        let r = self.alloc(1)?;
        self.expr_to_reg(start, r)?;
        let r = self.alloc(1)?;
        self.expr_to_reg(limit, r)?;
        let r = self.alloc(1)?;
        match step {
            Some(step) => self.expr_to_reg(step, r)?,
            None => {
                let k = self.constant(Value::Number(1.0))?;
                self.emit(Op::LoadK { a: r, k });
            }
        }
        self.f().line = line;
        let prep = self.emit(Op::ForPrep { a: base, offset: 0 });
        self.begin_block(true, false);
        let body_start = self.pc();
        let register = self.alloc(1)?;
        self.declare_local(var.clone(), register)?;
        self.block(body, false, false)?;
        self.f().line = line;
        let back = self.emit(Op::ForLoop { a: base, offset: 0 });
        self.patch(back, body_start);
        self.patch(prep, back + 1);
        self.end_block()?;
        self.set_free(u32::from(base));
        Ok(())
    }

    fn generic_for(
        &mut self,
        names: &[Name],
        values: &[Expr],
        body: &Block,
        (line, call_line): (u32, u32),
    ) -> Compile<()> {
        self.f().line = line;
        let base = self.alloc(0)?;
        self.expr_list(values, 3)?;
        self.set_free(u32::from(base) + 3);
        let to_call = self.jump();
        self.begin_block(true, false);
        let body_start = self.pc();
        let first = self.alloc(names.len() as u32)?;
        for (i, name) in names.iter().enumerate() {
            self.declare_local(name.clone(), first + i as u8)?;
        }
        self.block(body, false, false)?;
        let call = self.pc();
        // The loop's names end with its body: the iterator call after it is not theirs.
        let f = self.f();
        for local in &f.locals[f.locals.len() - names.len()..] {
            f.infos[local.info].end = call;
        }
        self.patch(to_call, call);
        self.f().line = call_line;
        let f = self.alloc(3)?;
        self.set_free(u32::from(f));
        self.emit(Op::Move {
            a: base + 3,
            b: base,
        });
        self.emit(Op::Move {
            a: base + 4,
            b: base + 1,
        });
        self.emit(Op::Move {
            a: base + 5,
            b: base + 2,
        });
        self.emit(Op::Call {
            func: base + 3,
            args: 3,
            results: names.len() as u8 + 1,
        });
        let back = self.emit(Op::ForEachLoop { a: base, offset: 0 });
        self.patch(back, body_start);
        self.end_block()?;
        self.set_free(u32::from(base));
        Ok(())
    }

    fn assign(&mut self, targets: &[Expr], values: &[Expr]) -> Compile<()> {
        let base = self.free();
        if let ([target], [value]) = (targets, values) {
            match &target.kind {
                ExprKind::Name(name) => match self.resolve(name)? {
                    Var::Local(register, false) => self.expr_to_reg(value, register)?,
                    Var::Local(register, true) => {
                        let v = self.expr_to_any(value)?;
                        self.emit(Op::SetCell {
                            cell: register,
                            b: v,
                        });
                    }
                    Var::Upvalue(upvalue) => {
                        let v = self.expr_to_any(value)?;
                        self.emit(Op::SetUpvalue { upvalue, b: v });
                    }
                    Var::Global(k) => {
                        let v = self.expr_to_any(value)?;
                        self.emit(Op::SetGlobal { b: v, k });
                    }
                },
                ExprKind::Index(table, key) => {
                    let table = self.expr_to_any(table)?;
                    let key = self.expr_to_rk(key)?;
                    let value = self.expr_to_rk(value)?;
                    self.emit(Op::SetTable { table, key, value });
                }
                _ => unreachable!("the parser only builds names and indexes as targets"),
            }
            self.set_free(base);
            return Ok(());
        }
        // Several targets: the targets' tables and keys first, then every value, then the stores right to left.
        let mut places = Vec::with_capacity(targets.len());
        for target in targets {
            places.push(match &target.kind {
                ExprKind::Name(name) => (self.resolve(name)?, None),
                ExprKind::Index(table, key) => {
                    let table = self.expr_to_next(table)?;
                    let key = self.expr_to_rk(key)?;
                    (Var::Local(table, false), Some(key))
                }
                _ => unreachable!("the parser only builds names and indexes as targets"),
            });
        }
        let first = self.alloc(0)?;
        self.expr_list(values, targets.len())?;
        for (i, (var, key)) in places.into_iter().enumerate().rev() {
            let source = first + i as u8;
            match (var, key) {
                (Var::Local(table, _), Some(key)) => {
                    self.emit(Op::SetTable {
                        table,
                        key,
                        value: Rk(u16::from(source)),
                    });
                }
                (Var::Local(register, false), None) => {
                    self.emit(Op::Move {
                        a: register,
                        b: source,
                    });
                }
                (Var::Local(register, true), None) => {
                    self.emit(Op::SetCell {
                        cell: register,
                        b: source,
                    });
                }
                (Var::Upvalue(upvalue), _) => {
                    self.emit(Op::SetUpvalue { upvalue, b: source });
                }
                (Var::Global(k), _) => {
                    self.emit(Op::SetGlobal { b: source, k });
                }
            }
        }
        self.set_free(base);
        Ok(())
    }

    fn ret(&mut self, values: &[Expr]) -> Compile<()> {
        if let [single] = values {
            if matches!(single.kind, ExprKind::Call(..) | ExprKind::Method(..)) {
                let base = self.alloc(0)?;
                let args = self.call_setup(single)?;
                self.emit(Op::TailCall { func: base, args });
                self.emit(Op::Return {
                    first: base,
                    count: 0,
                });
                return Ok(());
            }
            if let ExprKind::Name(name) = &single.kind
                && let Var::Local(register, false) = self.resolve(name)?
            {
                self.emit(Op::Return {
                    first: register,
                    count: 2,
                });
                return Ok(());
            }
        }
        let base = self.alloc(0)?;
        let open = values.last().is_some_and(Expr::is_multi);
        if open {
            self.expr_list_open(values)?;
            self.emit(Op::Return {
                first: base,
                count: 0,
            });
        } else {
            self.expr_list(values, values.len())?;
            self.emit(Op::Return {
                first: base,
                count: values.len() as u8 + 1,
            });
        }
        Ok(())
    }

    // --- expression lists ------------------------------------------------------------------------------------

    /// Evaluates `values` into `count` consecutive new registers (padding with nil, dropping extras).
    fn expr_list(&mut self, values: &[Expr], count: usize) -> Compile<()> {
        let base = self.free();
        for (i, value) in values.iter().enumerate() {
            let last = i + 1 == values.len();
            if last && value.is_multi() && i < count {
                let wanted = count - i;
                self.multi(value, Want::Fixed(u8::try_from(wanted).unwrap_or(u8::MAX)))?;
                self.set_free(base + count as u32);
                self.alloc(0)?;
                return Ok(());
            }
            self.expr_to_next(value)?;
        }
        let given = values.len();
        if given < count {
            let first = self.alloc((count - given) as u32)?;
            self.emit(Op::LoadNil {
                a: first,
                count: (count - given - 1) as u8,
            });
        }
        self.set_free(base + count as u32);
        Ok(())
    }

    /// Evaluates `values` into consecutive new registers, the last giving all its values (setting the top).
    fn expr_list_open(&mut self, values: &[Expr]) -> Compile<()> {
        let (last, rest) = values.split_last().expect("an open list has a last value");
        for value in rest {
            self.expr_to_next(value)?;
        }
        self.multi(last, Want::All)
    }

    // --- calls -----------------------------------------------------------------------------------------------

    /// Puts the function and its arguments in new registers from the current free one; returns the `args` field.
    fn call_setup(&mut self, e: &Expr) -> Compile<u8> {
        let base = self.free();
        let args = match &e.kind {
            ExprKind::Call(function, args) => {
                let r = self.alloc(1)?;
                self.expr_to_reg(function, r)?;
                args
            }
            ExprKind::Method(object, name, args) => {
                let object = self.expr_to_any(object)?;
                self.set_free(base);
                let a = self.alloc(2)?;
                let k = self.string_constant(name)?;
                let key = self.rk_constant(k)?;
                self.emit(Op::SelfMethod { a, object, key });
                self.set_free(base + 2);
                args
            }
            _ => unreachable!("a call"),
        };
        self.f().line = e.line;
        let open = args.last().is_some_and(Expr::is_multi);
        let field = if open {
            self.expr_list_open(args)?;
            0
        } else {
            for arg in args {
                self.expr_to_next(arg)?;
            }
            (self.free() - base) as u8
        };
        self.f().line = e.line;
        Ok(field)
    }

    /// Calls `e` with results from the current free register.
    fn call(&mut self, e: &Expr, want: Want) -> Compile<()> {
        let base = self.alloc(0)?;
        let args = self.call_setup(e)?;
        self.emit(Op::Call {
            func: base,
            args,
            results: want.encode(),
        });
        self.set_free(u32::from(base));
        Ok(())
    }

    /// A call or `...` giving `want` values from the current free register (`All` sets the top).
    fn multi(&mut self, e: &Expr, want: Want) -> Compile<()> {
        let base = self.free();
        match e.kind {
            ExprKind::Dots => {
                if !self.vararg() {
                    return Err(self.syntax(
                        e.line,
                        "cannot use '...' outside a vararg function near '...'",
                    ));
                }
                let a = self.alloc(0)?;
                self.emit(Op::VarArg {
                    a,
                    count: want.encode(),
                });
            }
            _ => self.call(e, want)?,
        }
        if let Want::Fixed(n) = want {
            self.set_free(base);
            self.alloc(u32::from(n))?;
        }
        Ok(())
    }

    fn vararg(&self) -> bool {
        self.functions
            .last()
            .expect("a function being compiled")
            .vararg
    }

    // --- single expressions ----------------------------------------------------------------------------------

    fn expr_to_next(&mut self, e: &Expr) -> Compile<u8> {
        let r = self.alloc(1)?;
        self.expr_to_reg(e, r)?;
        Ok(r)
    }

    /// A register holding `e`'s value: a plain local's own register, or a new one.
    fn expr_to_any(&mut self, e: &Expr) -> Compile<u8> {
        if let ExprKind::Name(name) = &e.kind
            && let Var::Local(register, false) = self.resolve(name)?
        {
            return Ok(register);
        }
        self.expr_to_next(e)
    }

    fn rk_constant(&mut self, k: u32) -> Compile<Rk> {
        if k <= Rk::MAX_CONSTANT {
            return Ok(Rk(Rk::CONSTANT + k as u16));
        }
        let r = self.alloc(1)?;
        self.emit(Op::LoadK { a: r, k });
        Ok(Rk(u16::from(r)))
    }

    fn expr_to_rk(&mut self, e: &Expr) -> Compile<Rk> {
        if let Some(value) = self.constant_of(e)? {
            let k = self.constant(value)?;
            return self.rk_constant(k);
        }
        Ok(Rk(u16::from(self.expr_to_any(e)?)))
    }

    /// Whether an expression that needs the registers above its target, or writes it before reading every operand,
    /// must go through a new register: unless the target is the newest register and no local's.
    fn needs_temp(&mut self, register: u8) -> bool {
        u32::from(register) + 1 != self.free()
            || self.f().locals.iter().any(|l| l.register == register)
    }

    fn expr_to_reg(&mut self, e: &Expr, reg: u8) -> Compile<()> {
        self.enter()?;
        let saved = self.free();
        self.f().line = e.line;
        if let Some(value) = self.constant_of(e)? {
            match value {
                Value::Nil => {
                    self.emit(Op::LoadNil { a: reg, count: 0 });
                }
                Value::Boolean(b) => {
                    self.emit(Op::LoadBool {
                        a: reg,
                        value: b,
                        skip: false,
                    });
                }
                _ => {
                    let k = self.constant(value)?;
                    self.emit(Op::LoadK { a: reg, k });
                }
            }
            self.set_free(saved);
            self.leave();
            return Ok(());
        }
        match &e.kind {
            ExprKind::Dots | ExprKind::Call(..) | ExprKind::Method(..) | ExprKind::Table(_)
                if self.needs_temp(reg) =>
            {
                let temp = self.alloc(1)?;
                self.expr_to_reg(e, temp)?;
                self.emit(Op::Move { a: reg, b: temp });
            }
            ExprKind::Dots => {
                self.set_free(u32::from(reg));
                self.multi(e, Want::Fixed(1))?;
            }
            ExprKind::Call(..) | ExprKind::Method(..) => {
                self.set_free(u32::from(reg));
                self.multi(e, Want::Fixed(1))?;
            }
            ExprKind::Table(fields) => self.table(fields, reg)?,
            ExprKind::Function(body) => self.closure(body, reg)?,
            ExprKind::Name(name) => match self.resolve(name)? {
                Var::Local(register, false) => {
                    if register != reg {
                        self.emit(Op::Move {
                            a: reg,
                            b: register,
                        });
                    }
                }
                Var::Local(register, true) => {
                    self.emit(Op::GetCell {
                        a: reg,
                        cell: register,
                    });
                }
                Var::Upvalue(upvalue) => {
                    self.emit(Op::GetUpvalue { a: reg, upvalue });
                }
                Var::Global(k) => {
                    self.emit(Op::GetGlobal { a: reg, k });
                }
            },
            ExprKind::Index(table, key) => {
                let table = self.expr_to_any(table)?;
                let key = self.expr_to_rk(key)?;
                self.f().line = e.line;
                self.emit(Op::GetTable { a: reg, table, key });
            }
            ExprKind::Paren(inner) => self.expr_to_reg(inner, reg)?,
            ExprKind::Unary(op, inner) => {
                let b = self.expr_to_any(inner)?;
                self.f().line = e.line;
                self.emit(match op {
                    UnOp::Neg => Op::Neg { a: reg, b },
                    UnOp::Not => Op::Not { a: reg, b },
                    UnOp::Len => Op::Len { a: reg, b },
                });
            }
            ExprKind::Binary(op, _, _) => match op {
                BinOp::And | BinOp::Or => self.logical_to_reg(e, *op, reg)?,
                BinOp::Concat => self.concat_to_reg(e, reg)?,
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    let jumps = self.jump_if(e, true)?;
                    self.emit(Op::LoadBool {
                        a: reg,
                        value: false,
                        skip: true,
                    });
                    self.patch_here(jumps);
                    self.emit(Op::LoadBool {
                        a: reg,
                        value: true,
                        skip: false,
                    });
                }
                _ => self.arith_to_reg(e, reg)?,
            },
            ExprKind::Nil
            | ExprKind::True
            | ExprKind::False
            | ExprKind::Number(_)
            | ExprKind::String(_) => unreachable!("folded above"),
        }
        self.set_free(saved);
        self.leave();
        Ok(())
    }

    /// `a op b op c ...` for the arithmetic operators, as a loop down the left spine.
    fn arith_to_reg(&mut self, e: &Expr, reg: u8) -> Compile<()> {
        let mut spine = Vec::new();
        let mut node = e;
        while let ExprKind::Binary(op, left, right) = &node.kind {
            let Some(arith) = is_arith(*op) else { break };
            spine.push((arith, right.as_ref(), node.line));
            node = left;
        }
        let saved = self.free();
        let mut accumulator = self.expr_to_rk(node)?;
        let temp = if spine.len() > 1 {
            Some(self.alloc(1)?)
        } else {
            None
        };
        let steps = spine.len();
        for (i, (op, right, line)) in spine.into_iter().rev().enumerate() {
            let c = self.expr_to_rk(right)?;
            self.f().line = line;
            let a = if i + 1 == steps {
                reg
            } else {
                temp.expect("a temporary for a chain")
            };
            self.emit(Op::Arith {
                op,
                a,
                b: accumulator,
                c,
            });
            accumulator = Rk(u16::from(a));
            let keep = temp.map_or(saved, |t| u32::from(t) + 1);
            self.set_free(keep);
        }
        self.set_free(saved);
        Ok(())
    }

    /// `a .. b .. c ...` (right-associative): every operand in consecutive registers, one `Concat`.
    fn concat_to_reg(&mut self, e: &Expr, reg: u8) -> Compile<()> {
        let mut operands = Vec::new();
        let mut node = e;
        while let ExprKind::Binary(BinOp::Concat, left, right) = &node.kind {
            operands.push(left.as_ref());
            node = right;
        }
        operands.push(node);
        let first = self.free() as u8;
        for operand in &operands {
            self.expr_to_next(operand)?;
        }
        let last = first + (operands.len() - 1) as u8;
        self.f().line = e.line;
        self.emit(Op::Concat {
            a: reg,
            first,
            last,
        });
        Ok(())
    }

    /// `a and b and ...` or `a or b or ...` into `reg`, as a loop down the left spine.
    fn logical_to_reg(&mut self, e: &Expr, op: BinOp, reg: u8) -> Compile<()> {
        let mut rights = Vec::new();
        let mut node = e;
        while let ExprKind::Binary(o, left, right) = &node.kind {
            if *o != op {
                break;
            }
            rights.push(right.as_ref());
            node = left;
        }
        let target = if self.needs_temp(reg) {
            self.alloc(1)?
        } else {
            reg
        };
        self.expr_to_reg(node, target)?;
        let mut ends = Vec::new();
        for right in rights.into_iter().rev() {
            // `and` stops at the first false value, `or` at the first true one.
            self.emit(Op::Test {
                a: target,
                expect: op == BinOp::Or,
            });
            ends.push(self.jump());
            self.expr_to_reg(right, target)?;
        }
        self.patch_here(ends);
        if target != reg {
            self.emit(Op::Move { a: reg, b: target });
        }
        Ok(())
    }

    /// Code that jumps (to the returned jumps, patched by the caller) exactly when `e`'s truth is `when`, and
    /// falls through otherwise.
    fn jump_if(&mut self, e: &Expr, when: bool) -> Compile<Vec<usize>> {
        self.enter()?;
        let saved = self.free();
        self.f().line = e.line;
        let jumps = match &e.kind {
            ExprKind::Nil | ExprKind::False => {
                if when {
                    Vec::new()
                } else {
                    vec![self.jump()]
                }
            }
            ExprKind::True | ExprKind::Number(_) | ExprKind::String(_) => {
                if when {
                    vec![self.jump()]
                } else {
                    Vec::new()
                }
            }
            ExprKind::Unary(UnOp::Not, inner) => self.jump_if(inner, !when)?,
            ExprKind::Paren(inner) => self.jump_if(inner, when)?,
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), _, _) => {
                let mut operands = Vec::new();
                let mut node = e;
                while let ExprKind::Binary(o, left, right) = &node.kind {
                    if o != op {
                        break;
                    }
                    operands.push(right.as_ref());
                    node = left;
                }
                operands.push(node);
                operands.reverse();
                // `or` jumps out when an operand is true; `and` when one is false.
                let decisive = *op == BinOp::Or;
                let (last, rest) = operands.split_last().expect("two operands at least");
                let mut out = Vec::new();
                let mut past = Vec::new();
                for operand in rest {
                    let jumps = self.jump_if(operand, decisive)?;
                    if decisive == when {
                        out.extend(jumps);
                    } else {
                        past.extend(jumps);
                    }
                }
                out.extend(self.jump_if(last, when)?);
                self.patch_here(past);
                out
            }
            ExprKind::Binary(
                op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge),
                left,
                right,
            ) => {
                let (compare, swap, negate) = match op {
                    BinOp::Eq => (CompareOp::Eq, false, false),
                    BinOp::Ne => (CompareOp::Eq, false, true),
                    BinOp::Lt => (CompareOp::Lt, false, false),
                    BinOp::Le => (CompareOp::Le, false, false),
                    BinOp::Gt => (CompareOp::Lt, true, false),
                    _ => (CompareOp::Le, true, false),
                };
                let b = self.expr_to_rk(left)?;
                let c = self.expr_to_rk(right)?;
                let (b, c) = if swap { (c, b) } else { (b, c) };
                self.f().line = e.line;
                self.emit(Op::Compare {
                    op: compare,
                    b,
                    c,
                    expect: when != negate,
                });
                vec![self.jump()]
            }
            _ => {
                let a = self.expr_to_any(e)?;
                self.emit(Op::Test { a, expect: when });
                vec![self.jump()]
            }
        };
        self.set_free(saved);
        self.leave();
        Ok(jumps)
    }

    fn table(&mut self, fields: &[Field], reg: u8) -> Compile<()> {
        let array = fields
            .iter()
            .filter(|f| matches!(f, Field::Positional(_)))
            .count();
        let hash = fields.len() - array;
        let pc = self.emit(Op::NewTable {
            a: reg,
            array: array.min(u16::MAX as usize) as u16,
            hash: hash.min(u16::MAX as usize) as u16,
        });
        let _ = pc;
        let mut pending = 0u32;
        let mut stored = 0u32;
        for (i, field) in fields.iter().enumerate() {
            match field {
                Field::Positional(value) => {
                    let last = i + 1 == fields.len();
                    if last && value.is_multi() {
                        self.multi(value, Want::All)?;
                        self.emit(Op::SetList {
                            table: reg,
                            count: 0,
                            start: stored + 1,
                        });
                        self.set_free(u32::from(reg) + 1);
                        return Ok(());
                    }
                    self.expr_to_next(value)?;
                    pending += 1;
                    if pending == FIELDS_PER_FLUSH {
                        self.emit(Op::SetList {
                            table: reg,
                            count: pending as u8 + 1,
                            start: stored + 1,
                        });
                        stored += pending;
                        pending = 0;
                        self.set_free(u32::from(reg) + 1);
                    }
                }
                Field::Keyed(key, value) => {
                    // Store in source order, as LuaJIT's constructors do: positional items first.
                    if pending > 0 {
                        self.emit(Op::SetList {
                            table: reg,
                            count: pending as u8 + 1,
                            start: stored + 1,
                        });
                        stored += pending;
                        pending = 0;
                        self.set_free(u32::from(reg) + 1);
                    }
                    let saved = self.free();
                    let key = self.expr_to_rk(key)?;
                    let value = self.expr_to_rk(value)?;
                    self.emit(Op::SetTable {
                        table: reg,
                        key,
                        value,
                    });
                    self.set_free(saved);
                }
            }
        }
        if pending > 0 {
            self.emit(Op::SetList {
                table: reg,
                count: pending as u8 + 1,
                start: stored + 1,
            });
        }
        self.set_free(u32::from(reg) + 1);
        Ok(())
    }

    fn closure(&mut self, body: &FunctionBody, reg: u8) -> Compile<()> {
        let proto = self.function_proto(
            &body.params,
            body.vararg,
            &body.body,
            body.line,
            body.end_line,
        )?;
        let f = self.f();
        let index = f.protos.len() as u32;
        f.protos.push(proto);
        f.line = body.line;
        self.emit(Op::Closure {
            a: reg,
            proto: index,
        });
        Ok(())
    }
}
