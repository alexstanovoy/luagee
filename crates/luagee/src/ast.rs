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
