//! The syntax tree the parser builds and the compiler reads.

pub(crate) type Name = Box<[u8]>;

#[derive(Debug, Default)]
pub(crate) struct Block {
    pub(crate) statements: Vec<Statement>,
    /// The final `return`, with its line.
    pub(crate) ret: Option<(Vec<Expr>, u32)>,
}

#[derive(Debug)]
pub(crate) enum Statement {
    /// A function call used as a statement.
    Call(Expr),
    Local {
        names: Vec<Name>,
        values: Vec<Expr>,
        line: u32,
    },
    Assign {
        targets: Vec<Expr>,
        values: Vec<Expr>,
        line: u32,
    },
    Do(Block),
    While {
        condition: Expr,
        body: Block,
    },
    Repeat {
        body: Block,
        condition: Expr,
    },
    If {
        arms: Vec<(Expr, Block)>,
        otherwise: Option<Block>,
    },
    NumericFor {
        var: Name,
        start: Expr,
        limit: Expr,
        step: Option<Expr>,
        body: Block,
        line: u32,
    },
    GenericFor {
        names: Vec<Name>,
        values: Vec<Expr>,
        body: Block,
        line: u32,
    },
    LocalFunction {
        name: Name,
        body: Box<FunctionBody>,
    },
    Break {
        line: u32,
    },
    Goto {
        label: Name,
        line: u32,
    },
    Label {
        name: Name,
        line: u32,
    },
}

#[derive(Debug)]
pub(crate) struct FunctionBody {
    pub(crate) params: Vec<Name>,
    pub(crate) vararg: bool,
    pub(crate) body: Block,
    pub(crate) line: u32,
    pub(crate) end_line: u32,
}

#[derive(Debug)]
pub(crate) struct Expr {
    pub(crate) kind: ExprKind,
    pub(crate) line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    Not,
    Len,
}

#[derive(Debug)]
pub(crate) enum ExprKind {
    Nil,
    True,
    False,
    Dots,
    Number(f64),
    String(Box<[u8]>),
    Function(Box<FunctionBody>),
    Table(Vec<Field>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Name(Name),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Method(Box<Expr>, Name, Vec<Expr>),
    Paren(Box<Expr>),
}

#[derive(Debug)]
pub(crate) enum Field {
    Positional(Expr),
    Keyed(Expr, Expr),
}

impl Expr {
    /// Whether the expression can give several values (a call or `...` not in parentheses).
    pub(crate) fn is_multi(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::Call(..) | ExprKind::Method(..) | ExprKind::Dots
        )
    }
}

/// Drops an expression tree with a worklist instead of recursion: a chain of a million `+` nodes is a million boxes
/// deep, and dropping it recursively would run out of native stack (wasm's above all).
impl Drop for Expr {
    fn drop(&mut self) {
        let mut pending: Vec<Expr> = Vec::new();
        take_children(&mut self.kind, &mut pending);
        while let Some(mut expr) = pending.pop() {
            take_children(&mut expr.kind, &mut pending);
            // `expr` drops here with no expression children left: no recursion.
        }
    }
}

/// Moves an expression's child expressions out into `pending`, leaving it a leaf.
fn take_children(kind: &mut ExprKind, pending: &mut Vec<Expr>) {
    match std::mem::replace(kind, ExprKind::Nil) {
        ExprKind::Binary(_, left, right) | ExprKind::Index(left, right) => {
            pending.push(*left);
            pending.push(*right);
        }
        ExprKind::Unary(_, inner) | ExprKind::Paren(inner) => pending.push(*inner),
        ExprKind::Call(function, args) => {
            pending.push(*function);
            pending.extend(args);
        }
        ExprKind::Method(object, _, args) => {
            pending.push(*object);
            pending.extend(args);
        }
        ExprKind::Table(fields) => {
            for field in fields {
                match field {
                    Field::Positional(value) => pending.push(value),
                    Field::Keyed(key, value) => {
                        pending.push(key);
                        pending.push(value);
                    }
                }
            }
        }
        // A function body's statements nest only as deep as the parser allowed.
        ExprKind::Function(_)
        | ExprKind::Nil
        | ExprKind::True
        | ExprKind::False
        | ExprKind::Dots
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Name(_) => {}
    }
}
