//! ## Vex [Vexlang] ##
//! # Version: 0.0.1 (SemVer)
//! # Inspired by Rust & Zig
//! # Built for Cybercore ecosystem code consistency and ease
//! # GitHub: [Vexlang] (https://github.com/cybercoretech/vexlang)
//! # Tag reference doc in /home/raven/devspace/docs/tags/TAG_API.md
//!

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Inferred,
    I32,
    U64,
    F64,
    Bool,
    String,
    Unit,
    Res, // The unique Vex resource type
    Custom(String),
    Array(Box<Type>),
}

/// A trait method signature: method name, parameters, and return type.
pub type MethodSig = (String, Vec<(String, Type)>, Type);

#[derive(Debug, Clone, PartialEq)] // Add PartialEq here
pub enum Op {
    And,
    Add,
    Sub,
    Mul,
    Div,
    Or,
    Eq,
    Lt,
    Gt,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    String(String),
    Bool(bool),
    Var(String),
    BinaryOp(Box<Expr>, Op, Box<Expr>), // The core of arithmetic
    UnaryOp(Op, Box<Expr>),
    Block(Vec<Stmt>, Option<Box<Expr>>),
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    Call(String, Vec<Expr>),
    Record(String, Vec<(String, Expr)>),
    Field(Box<Expr>, String),
    Array(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Try(Box<Expr>),
    Match {
        value: Box<Expr>,
        arms: Vec<(Pattern, Expr)>,
    },
    EnumVariant {
        enum_name: String,
        variant: String,
        fields: Vec<(String, Expr)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Enum {
        enum_name: Option<String>,
        variant: String,
        bindings: Vec<String>,
    },
    Result {
        ok: bool,
        binding: Option<String>,
    },
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Function {
        name: String,
        params: Vec<(String, Type)>,
        return_type: Type,
        body: Expr,
    },
    Record {
        name: String,
        fields: Vec<(String, Type)>,
    },
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
    },
    Let {
        name: String,
        ty: Type,
        value: Expr,
    },
    Assign {
        name: String,
        value: Expr,
    },
    AssignIndex {
        name: String,
        indices: Vec<Expr>,
        value: Expr,
    },
    ExprStmt(Expr),
    If {
        condition: Expr,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    While {
        condition: Expr,
        body: Box<Expr>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    // Custom language constructs
    Trait {
        name: String,
        methods: Vec<MethodSig>,
    },
    Impl {
        trait_name: Option<String>,
        target: String,
        methods: Vec<Stmt>,
    },
    Module {
        name: String,
        body: Vec<Stmt>,
    },
    Use {
        path: Vec<String>,
    },
    Export {
        name: String,
    },
    Contract {
        name: String,
        invariants: Vec<Expr>,
    },
    Invariant(Expr),
    Newtype {
        name: String,
        underlying: Type,
    },
    Test(Box<Stmt>),
    Bench(Box<Stmt>),
    Extern {
        abi: Option<String>,
        func: Box<Stmt>,
    },
    Unsafe(Expr),
    Trusted(Box<Stmt>),
    Ensure {
        condition: Expr,
        message: Expr,
    },
    Throw(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    U64(u64),
    Bool(bool),
    String(String),
    Unit,
    Record(String, BTreeMap<String, Value>),
    Array(Vec<Value>),
    Enum {
        enum_name: String,
        variant: String,
        fields: BTreeMap<String, Value>,
    },
    Result {
        ok: bool,
        value: Box<Value>,
    },
    Map(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}
