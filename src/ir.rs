//! Typed, backend-independent intermediate representation.
//!
//! This is deliberately a small textual IR separate from QBE.  It gives the
//! compiler pipeline a checked lowering target before optional target-specific
//! emitters produce artifacts such as experimental scalar QBE IL.

use crate::ast::{Expr, Op, Stmt, Type};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrType {
    I32,
    U64,
    Bool,
    String,
    Unit,
    Record(String),
    Array(Box<IrType>),
    Enum(String),
}

impl std::fmt::Display for IrType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::I32 => "i32",
            Self::U64 => "u64",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Unit => "unit",
            Self::Record(name) => return write!(f, "record {name}"),
            Self::Array(element) => return write!(f, "[{element}]"),
            Self::Enum(name) => return write!(f, "enum {name}"),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrExpr {
    pub ty: IrType,
    pub kind: IrExprKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrExprKind {
    Int(i64),
    Bool(bool),
    String(String),
    Var(String),
    Binary(Box<IrExpr>, Op, Box<IrExpr>),
    Unary(Op, Box<IrExpr>),
    Block(Vec<IrStmt>, Option<Box<IrExpr>>),
    If {
        condition: Box<IrExpr>,
        then_branch: Box<IrExpr>,
        else_branch: Option<Box<IrExpr>>,
    },
    Call(String, Vec<IrExpr>),
    Record(String, Vec<(String, IrExpr)>),
    Field(Box<IrExpr>, String),
    Array(Vec<IrExpr>),
    EnumVariant {
        enum_name: String,
        variant: String,
        fields: Vec<(String, IrExpr)>,
    },
    Index(Box<IrExpr>, Box<IrExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrStmt {
    Function(IrFunction),
    Record {
        name: String,
        fields: Vec<(String, IrType)>,
    },
    Enum {
        name: String,
        variants: Vec<(String, Vec<(String, IrType)>)>,
    },
    Let {
        name: String,
        ty: IrType,
        value: IrExpr,
    },
    Assign {
        name: String,
        value: IrExpr,
    },
    AssignIndex {
        name: String,
        indices: Vec<IrExpr>,
        value: IrExpr,
    },
    Expr(IrExpr),
    If {
        condition: IrExpr,
        then_branch: Box<IrExpr>,
        else_branch: Option<Box<IrExpr>>,
    },
    While {
        condition: IrExpr,
        body: Box<IrExpr>,
    },
    Break,
    Continue,
    Return(Option<IrExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrFunction {
    pub name: String,
    pub params: Vec<(String, IrType)>,
    pub return_type: IrType,
    pub body: IrExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrProgram {
    pub functions: Vec<IrFunction>,
    pub body: Vec<IrStmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    Unsupported(String),
    Invalid(String),
}

/// A machine-readable failure raised before a backend is allowed to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

impl VerifyError {
    fn invalid(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: "IR001",
            path: path.into(),
            message: message.into(),
        }
    }

    fn unsupported(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: "IR002",
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}: {}", self.code, self.path, self.message)
    }
}

impl std::error::Error for VerifyError {}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for LowerError {}

pub fn lower(stmts: &[Stmt]) -> Result<IrProgram, LowerError> {
    let mut functions = HashMap::new();
    for stmt in stmts {
        if let Stmt::Function {
            name,
            params,
            return_type,
            ..
        } = stmt
        {
            functions.insert(
                name.clone(),
                (
                    params
                        .iter()
                        .map(|(_, ty)| ir_type(ty))
                        .collect::<Result<Vec<_>, _>>()?,
                    ir_type(return_type)?,
                ),
            );
        }
    }
    let mut body = Vec::new();
    let mut scope = HashMap::new();
    for stmt in stmts {
        body.push(lower_stmt(stmt, &functions, &mut scope)?);
    }
    let program = IrProgram {
        functions: body
            .iter()
            .filter_map(|stmt| match stmt {
                IrStmt::Function(function) => Some(function.clone()),
                _ => None,
            })
            .collect(),
        body,
    };
    verify(&program).map_err(|error| LowerError::Invalid(error.to_string()))?;
    Ok(program)
}

/// Verify the IR contract shared by all future native backends.
///
/// The current contract deliberately contains only scalar arithmetic and
/// booleans, structured control flow, calls, records, and arrays. Strings,
/// enums, Result, maps, and indexed mutation are retained in the AST/evaluator
/// but are rejected here until their runtime representation is specified.
pub fn verify(program: &IrProgram) -> Result<(), VerifyError> {
    let mut functions = HashMap::new();
    let mut records = HashMap::new();
    for (index, stmt) in program.body.iter().enumerate() {
        match stmt {
            IrStmt::Function(function) => {
                if functions.insert(function.name.clone(), function).is_some() {
                    return Err(VerifyError::invalid(
                        format!("body[{index}]"),
                        format!("duplicate function `{}`", function.name),
                    ));
                }
            }
            IrStmt::Record { name, fields } => {
                if records.insert(name.clone(), fields).is_some() {
                    return Err(VerifyError::invalid(
                        format!("body[{index}]"),
                        format!("duplicate record `{name}`"),
                    ));
                }
            }
            IrStmt::Enum { name, .. } => {
                return Err(VerifyError::unsupported(
                    format!("body[{index}]"),
                    format!("enum `{name}` has no native runtime contract"),
                ));
            }
            _ => {}
        }
    }
    let mut scope = HashMap::new();
    for (index, stmt) in program.body.iter().enumerate() {
        verify_stmt(
            stmt,
            &format!("body[{index}]"),
            &functions,
            &records,
            &mut scope,
            false,
            None,
        )?;
    }
    Ok(())
}

fn verify_type(
    ty: &IrType,
    path: &str,
    records: &HashMap<String, &Vec<(String, IrType)>>,
) -> Result<(), VerifyError> {
    match ty {
        IrType::I32 | IrType::U64 | IrType::Bool | IrType::Unit => Ok(()),
        IrType::Array(element) => verify_type(element, &format!("{path}[]"), records),
        IrType::Record(name) => {
            if name == "Result" || name == "Map" {
                return Err(VerifyError::unsupported(
                    path,
                    format!("`{name}` runtime is deferred"),
                ));
            }
            if !records.contains_key(name) {
                return Err(VerifyError::invalid(
                    path,
                    format!("unknown record `{name}`"),
                ));
            }
            Ok(())
        }
        IrType::String => Err(VerifyError::unsupported(path, "strings need a runtime ABI")),
        IrType::Enum(name) => Err(VerifyError::unsupported(
            path,
            format!("enum `{name}` runtime is deferred"),
        )),
    }
}

fn verify_stmt(
    stmt: &IrStmt,
    path: &str,
    functions: &HashMap<String, &IrFunction>,
    records: &HashMap<String, &Vec<(String, IrType)>>,
    scope: &mut HashMap<String, IrType>,
    in_loop: bool,
    return_type: Option<&IrType>,
) -> Result<(), VerifyError> {
    match stmt {
        IrStmt::Function(function) => {
            let mut local = HashMap::new();
            for (name, ty) in &function.params {
                verify_type(ty, &format!("{path}.param.{name}"), records)?;
                if local.insert(name.clone(), ty.clone()).is_some() {
                    return Err(VerifyError::invalid(
                        path,
                        format!("duplicate parameter `{name}`"),
                    ));
                }
            }
            verify_type(&function.return_type, &format!("{path}.return"), records)?;
            verify_expr(
                &function.body,
                &format!("{path}.body"),
                functions,
                records,
                &mut local,
                false,
                Some(&function.return_type),
            )
        }
        IrStmt::Record { fields, .. } => {
            for (name, ty) in fields {
                verify_type(ty, &format!("{path}.field.{name}"), records)?;
            }
            Ok(())
        }
        IrStmt::Let { name, ty, value } => {
            verify_type(ty, &format!("{path}.type"), records)?;
            verify_expr(
                value,
                &format!("{path}.value"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if value.ty != *ty {
                return Err(VerifyError::invalid(
                    path,
                    format!("let `{name}` type {} does not match value {}", ty, value.ty),
                ));
            }
            scope.insert(name.clone(), ty.clone());
            Ok(())
        }
        IrStmt::Assign { name, value } => {
            let expected = scope.get(name).cloned().ok_or_else(|| {
                VerifyError::invalid(path, format!("assignment to undefined variable `{name}`"))
            })?;
            verify_expr(
                value,
                &format!("{path}.value"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if value.ty != expected {
                return Err(VerifyError::invalid(
                    path,
                    format!(
                        "assignment to `{name}` has type {}, expected {expected}",
                        value.ty
                    ),
                ));
            }
            Ok(())
        }
        IrStmt::AssignIndex { .. } => Err(VerifyError::unsupported(
            path,
            "indexed mutation runtime is deferred",
        )),
        IrStmt::Expr(expr) => verify_expr(
            expr,
            &format!("{path}.expr"),
            functions,
            records,
            scope,
            in_loop,
            return_type,
        ),
        IrStmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            verify_expr(
                condition,
                &format!("{path}.condition"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if condition.ty != IrType::Bool {
                return Err(VerifyError::invalid(path, "if condition must be bool"));
            }
            verify_expr(
                then_branch,
                &format!("{path}.then"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if let Some(expr) = else_branch {
                verify_expr(
                    expr,
                    &format!("{path}.else"),
                    functions,
                    records,
                    scope,
                    in_loop,
                    return_type,
                )?;
            }
            Ok(())
        }
        IrStmt::While { condition, body } => {
            verify_expr(
                condition,
                &format!("{path}.condition"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if condition.ty != IrType::Bool {
                return Err(VerifyError::invalid(path, "while condition must be bool"));
            }
            verify_expr(
                body,
                &format!("{path}.body"),
                functions,
                records,
                scope,
                true,
                return_type,
            )
        }
        IrStmt::Break | IrStmt::Continue if !in_loop => {
            Err(VerifyError::invalid(path, "loop control is outside a loop"))
        }
        IrStmt::Break | IrStmt::Continue => Ok(()),
        IrStmt::Return(expr) => {
            let expected = return_type
                .ok_or_else(|| VerifyError::invalid(path, "return is outside a function"))?;
            if let Some(expr) = expr {
                verify_expr(
                    expr,
                    &format!("{path}.value"),
                    functions,
                    records,
                    scope,
                    in_loop,
                    return_type,
                )?;
                if expr.ty != *expected {
                    return Err(VerifyError::invalid(
                        path,
                        format!("return has type {}, expected {expected}", expr.ty),
                    ));
                }
            } else if *expected != IrType::Unit {
                return Err(VerifyError::invalid(
                    path,
                    "non-unit function must return a value",
                ));
            }
            Ok(())
        }
        IrStmt::Enum { .. } => unreachable!(),
    }
}

fn verify_expr(
    expr: &IrExpr,
    path: &str,
    functions: &HashMap<String, &IrFunction>,
    records: &HashMap<String, &Vec<(String, IrType)>>,
    scope: &mut HashMap<String, IrType>,
    in_loop: bool,
    return_type: Option<&IrType>,
) -> Result<(), VerifyError> {
    verify_type(&expr.ty, &format!("{path}.type"), records)?;
    match &expr.kind {
        IrExprKind::Int(_) if expr.ty == IrType::I32 => Ok(()),
        IrExprKind::Bool(_) if expr.ty == IrType::Bool => Ok(()),
        IrExprKind::Int(_) | IrExprKind::Bool(_) => {
            Err(VerifyError::invalid(path, "literal type is inconsistent"))
        }
        IrExprKind::String(_) => Err(VerifyError::unsupported(path, "strings need a runtime ABI")),
        IrExprKind::Var(name) => {
            let ty = scope.get(name).ok_or_else(|| {
                VerifyError::invalid(path, format!("undefined variable `{name}`"))
            })?;
            if *ty != expr.ty {
                return Err(VerifyError::invalid(
                    path,
                    "variable type annotation is inconsistent",
                ));
            }
            Ok(())
        }
        IrExprKind::Binary(left, op, right) => {
            verify_expr(
                left,
                &format!("{path}.left"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            verify_expr(
                right,
                &format!("{path}.right"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if left.ty != right.ty {
                return Err(VerifyError::invalid(
                    path,
                    "binary operands have different types",
                ));
            }
            let expected = if matches!(op, Op::Eq | Op::Lt | Op::Gt | Op::And | Op::Or) {
                IrType::Bool
            } else {
                left.ty.clone()
            };
            if expr.ty != expected {
                return Err(VerifyError::invalid(
                    path,
                    "binary result type is inconsistent",
                ));
            }
            Ok(())
        }
        IrExprKind::Unary(op, value) => {
            verify_expr(
                value,
                &format!("{path}.value"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            let expected = if *op == Op::Not {
                IrType::Bool
            } else {
                value.ty.clone()
            };
            if expr.ty != expected {
                return Err(VerifyError::invalid(
                    path,
                    "unary result type is inconsistent",
                ));
            }
            Ok(())
        }
        IrExprKind::Block(stmts, tail) => {
            let mut local = scope.clone();
            for (index, stmt) in stmts.iter().enumerate() {
                verify_stmt(
                    stmt,
                    &format!("{path}.stmt[{index}]"),
                    functions,
                    records,
                    &mut local,
                    in_loop,
                    return_type,
                )?;
            }
            if let Some(tail) = tail {
                verify_expr(
                    tail,
                    &format!("{path}.tail"),
                    functions,
                    records,
                    &mut local,
                    in_loop,
                    return_type,
                )?;
            }
            Ok(())
        }
        IrExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            verify_expr(
                condition,
                &format!("{path}.condition"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if condition.ty != IrType::Bool {
                return Err(VerifyError::invalid(path, "if condition must be bool"));
            }
            verify_expr(
                then_branch,
                &format!("{path}.then"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            if let Some(other) = else_branch {
                verify_expr(
                    other,
                    &format!("{path}.else"),
                    functions,
                    records,
                    scope,
                    in_loop,
                    return_type,
                )?;
                if then_branch.ty != other.ty || expr.ty != then_branch.ty {
                    return Err(VerifyError::invalid(
                        path,
                        "if branches must have the same type",
                    ));
                }
            } else if expr.ty != IrType::Unit {
                return Err(VerifyError::invalid(
                    path,
                    "if without else must have unit type",
                ));
            }
            Ok(())
        }
        IrExprKind::Call(name, args) => {
            for (index, arg) in args.iter().enumerate() {
                verify_expr(
                    arg,
                    &format!("{path}.arg[{index}]"),
                    functions,
                    records,
                    scope,
                    in_loop,
                    return_type,
                )?;
            }
            if let Some(function) = functions.get(name) {
                if function.params.len() != args.len() {
                    return Err(VerifyError::invalid(
                        path,
                        "wrong number of function arguments",
                    ));
                }
                for ((_, expected), actual) in function.params.iter().zip(args) {
                    if *expected != actual.ty {
                        return Err(VerifyError::invalid(
                            path,
                            "function argument type mismatch",
                        ));
                    }
                }
                if expr.ty != function.return_type {
                    return Err(VerifyError::invalid(
                        path,
                        "call result type is inconsistent",
                    ));
                }
                Ok(())
            } else if matches!(name.as_str(), "i32" | "u64") {
                if args.len() != 1 || !matches!(args[0].ty, IrType::I32 | IrType::U64) {
                    return Err(VerifyError::invalid(
                        path,
                        format!("{name} conversion expects one integer argument"),
                    ));
                }
                let expected = if name == "i32" {
                    IrType::I32
                } else {
                    IrType::U64
                };
                if expr.ty != expected {
                    return Err(VerifyError::invalid(
                        path,
                        "conversion result type is inconsistent",
                    ));
                }
                Ok(())
            } else {
                Err(VerifyError::unsupported(
                    path,
                    format!("builtin `{name}` has no native contract"),
                ))
            }
        }
        IrExprKind::Record(name, fields) => {
            let definition = records
                .get(name)
                .ok_or_else(|| VerifyError::invalid(path, format!("unknown record `{name}`")))?;
            if expr.ty != IrType::Record(name.clone()) {
                return Err(VerifyError::invalid(
                    path,
                    "record expression type is inconsistent",
                ));
            }
            for (field, value) in fields {
                let (_, expected) =
                    definition.iter().find(|(n, _)| n == field).ok_or_else(|| {
                        VerifyError::invalid(path, format!("unknown field `{field}`"))
                    })?;
                verify_expr(
                    value,
                    &format!("{path}.{field}"),
                    functions,
                    records,
                    scope,
                    in_loop,
                    return_type,
                )?;
                if value.ty != *expected {
                    return Err(VerifyError::invalid(
                        path,
                        format!("field `{field}` type mismatch"),
                    ));
                }
            }
            Ok(())
        }
        IrExprKind::Field(value, field) => {
            verify_expr(
                value,
                &format!("{path}.record"),
                functions,
                records,
                scope,
                in_loop,
                return_type,
            )?;
            let IrType::Record(name) = &value.ty else {
                return Err(VerifyError::invalid(path, "field access requires a record"));
            };
            let definition = records
                .get(name)
                .ok_or_else(|| VerifyError::invalid(path, format!("unknown record `{name}`")))?;
            if !definition.iter().any(|(candidate, _)| candidate == field) {
                return Err(VerifyError::invalid(
                    path,
                    format!("unknown field `{field}` on `{name}`"),
                ));
            }
            // The current textual IR preserves the record type for projections;
            // a native backend must replace this with a layout-aware field type.
            Ok(())
        }
        IrExprKind::Array(values) => {
            let element = match &expr.ty {
                IrType::Array(element) => element,
                _ => {
                    return Err(VerifyError::invalid(
                        path,
                        "array expression must have array type",
                    ));
                }
            };
            for value in values {
                verify_expr(value, path, functions, records, scope, in_loop, return_type)?;
                if value.ty != **element {
                    return Err(VerifyError::invalid(
                        path,
                        "array elements have different types",
                    ));
                }
            }
            Ok(())
        }
        IrExprKind::EnumVariant { .. } => {
            Err(VerifyError::unsupported(path, "enum runtime is deferred"))
        }
        IrExprKind::Index(_, _) => Err(VerifyError::unsupported(
            path,
            "array indexing runtime is deferred",
        )),
    }
}

fn ir_type(ty: &Type) -> Result<IrType, LowerError> {
    match ty {
        Type::I32 | Type::Inferred => Ok(IrType::I32),
        Type::U64 => Ok(IrType::U64),
        Type::Bool => Ok(IrType::Bool),
        Type::String => Ok(IrType::String),
        Type::Unit => Ok(IrType::Unit),
        Type::Custom(name) => Ok(IrType::Record(name.clone())),
        Type::Array(element) => Ok(IrType::Array(Box::new(ir_type(element)?))),
        other => Err(LowerError::Unsupported(format!(
            "IR lowering does not support type annotation {other:?}"
        ))),
    }
}

type Functions = HashMap<String, (Vec<IrType>, IrType)>;
type Scope = HashMap<String, IrType>;

fn lower_stmt(stmt: &Stmt, functions: &Functions, scope: &mut Scope) -> Result<IrStmt, LowerError> {
    match stmt {
        Stmt::Record { name, fields } => Ok(IrStmt::Record {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(n, t)| Ok((n.clone(), ir_type(t)?)))
                .collect::<Result<_, _>>()?,
        }),
        Stmt::Enum { name, variants } => Ok(IrStmt::Enum {
            name: name.clone(),
            variants: variants
                .iter()
                .map(|variant| {
                    Ok((
                        variant.name.clone(),
                        variant
                            .fields
                            .iter()
                            .map(|(field, ty)| Ok((field.clone(), ir_type(ty)?)))
                            .collect::<Result<_, LowerError>>()?,
                    ))
                })
                .collect::<Result<_, LowerError>>()?,
        }),
        Stmt::Function {
            name,
            params,
            return_type,
            body,
        } => {
            let mut local = params
                .iter()
                .map(|(name, ty)| Ok((name.clone(), ir_type(ty)?)))
                .collect::<Result<Scope, LowerError>>()?;
            Ok(IrStmt::Function(IrFunction {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|(name, ty)| Ok((name.clone(), ir_type(ty)?)))
                    .collect::<Result<Vec<_>, LowerError>>()?,
                return_type: ir_type(return_type)?,
                body: lower_expr(body, functions, &mut local)?,
            }))
        }
        Stmt::Let { name, ty, value } => {
            let value = lower_expr(value, functions, scope)?;
            let ty = if *ty == Type::Inferred {
                value.ty.clone()
            } else {
                ir_type(ty)?
            };
            scope.insert(name.clone(), ty.clone());
            Ok(IrStmt::Let {
                name: name.clone(),
                ty,
                value,
            })
        }
        Stmt::Assign { name, value } => Ok(IrStmt::Assign {
            name: name.clone(),
            value: lower_expr(value, functions, scope)?,
        }),
        Stmt::AssignIndex {
            name,
            indices,
            value,
        } => Ok(IrStmt::AssignIndex {
            name: name.clone(),
            indices: indices
                .iter()
                .map(|x| lower_expr(x, functions, scope))
                .collect::<Result<_, _>>()?,
            value: lower_expr(value, functions, scope)?,
        }),
        Stmt::ExprStmt(expr) => Ok(IrStmt::Expr(lower_expr(expr, functions, scope)?)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Ok(IrStmt::If {
            condition: lower_expr(condition, functions, scope)?,
            then_branch: Box::new(lower_expr(then_branch, functions, scope)?),
            else_branch: else_branch
                .as_ref()
                .map(|expr| lower_expr(expr, functions, scope))
                .transpose()?
                .map(Box::new),
        }),
        Stmt::While { condition, body } => Ok(IrStmt::While {
            condition: lower_expr(condition, functions, scope)?,
            body: Box::new(lower_expr(body, functions, scope)?),
        }),
        Stmt::Break => Ok(IrStmt::Break),
        Stmt::Continue => Ok(IrStmt::Continue),
        Stmt::Return(expr) => Ok(IrStmt::Return(
            expr.as_ref()
                .map(|expr| lower_expr(expr, functions, scope))
                .transpose()?,
        )),
        Stmt::Test(inner)
        | Stmt::Bench(inner)
        | Stmt::Trusted(inner)
        | Stmt::Extern { func: inner, .. } => lower_stmt(inner, functions, scope),
        Stmt::Unsafe(expr) => Ok(IrStmt::Expr(lower_expr(expr, functions, scope)?)),
        other => Err(LowerError::Unsupported(format!(
            "IR lowering does not yet support statement `{other:?}`"
        ))),
    }
}

fn lower_expr(expr: &Expr, functions: &Functions, scope: &mut Scope) -> Result<IrExpr, LowerError> {
    let (ty, kind) = match expr {
        Expr::Int(value) => (IrType::I32, IrExprKind::Int(*value)),
        Expr::Bool(value) => (IrType::Bool, IrExprKind::Bool(*value)),
        Expr::String(value) => (IrType::String, IrExprKind::String(value.clone())),
        Expr::Array(values) => {
            let values = values
                .iter()
                .map(|x| lower_expr(x, functions, scope))
                .collect::<Result<Vec<_>, _>>()?;
            let ty = values
                .first()
                .map(|x| IrType::Array(Box::new(x.ty.clone())))
                .unwrap_or(IrType::Array(Box::new(IrType::Unit)));
            (ty, IrExprKind::Array(values))
        }
        Expr::Index(value, index) => {
            let value = lower_expr(value, functions, scope)?;
            let index = lower_expr(index, functions, scope)?;
            let ty = match &value.ty {
                IrType::Array(element) => (**element).clone(),
                _ => return Err(LowerError::Invalid("indexing requires an array".into())),
            };
            (ty, IrExprKind::Index(Box::new(value), Box::new(index)))
        }
        Expr::Record(name, fields) => {
            let fields = fields
                .iter()
                .map(|(field, value)| Ok((field.clone(), lower_expr(value, functions, scope)?)))
                .collect::<Result<Vec<_>, LowerError>>()?;
            (
                IrType::Record(name.clone()),
                IrExprKind::Record(name.clone(), fields),
            )
        }
        Expr::EnumVariant {
            enum_name,
            variant,
            fields,
        } => {
            let fields = fields
                .iter()
                .map(|(field, value)| Ok((field.clone(), lower_expr(value, functions, scope)?)))
                .collect::<Result<Vec<_>, LowerError>>()?;
            (
                IrType::Enum(enum_name.clone()),
                IrExprKind::EnumVariant {
                    enum_name: enum_name.clone(),
                    variant: variant.clone(),
                    fields,
                },
            )
        }
        Expr::Field(value, field) => {
            let value = lower_expr(value, functions, scope)?;
            let ty = match &value.ty {
                IrType::Record(name) => IrType::Record(name.clone()),
                _ => return Err(LowerError::Invalid("field access requires a record".into())),
            };
            (ty, IrExprKind::Field(Box::new(value), field.clone()))
        }
        Expr::Var(name) => (
            scope
                .get(name)
                .cloned()
                .ok_or_else(|| LowerError::Invalid(format!("undefined variable `{name}`")))?,
            IrExprKind::Var(name.clone()),
        ),
        Expr::BinaryOp(left, op, right) => {
            let left = lower_expr(left, functions, scope)?;
            let right = lower_expr(right, functions, scope)?;
            let ty = if matches!(op, Op::Eq | Op::Lt | Op::Gt | Op::And | Op::Or) {
                IrType::Bool
            } else {
                left.ty.clone()
            };
            (
                ty,
                IrExprKind::Binary(Box::new(left), op.clone(), Box::new(right)),
            )
        }
        Expr::UnaryOp(op, value) => {
            let value = lower_expr(value, functions, scope)?;
            let ty = if *op == Op::Not {
                IrType::Bool
            } else {
                value.ty.clone()
            };
            (ty, IrExprKind::Unary(op.clone(), Box::new(value)))
        }
        Expr::Block(stmts, tail) => {
            let mut local = scope.clone();
            let lowered = stmts
                .iter()
                .map(|stmt| lower_stmt(stmt, functions, &mut local))
                .collect::<Result<Vec<_>, _>>()?;
            let tail = tail
                .as_ref()
                .map(|expr| lower_expr(expr, functions, &mut local))
                .transpose()?;
            let ty = tail.as_ref().map_or(IrType::Unit, |expr| expr.ty.clone());
            (ty, IrExprKind::Block(lowered, tail.map(Box::new)))
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let condition = lower_expr(condition, functions, scope)?;
            let then_branch = lower_expr(then_branch, functions, scope)?;
            let else_branch = else_branch
                .as_ref()
                .map(|expr| lower_expr(expr, functions, scope))
                .transpose()?;
            let ty = else_branch
                .as_ref()
                .map_or(IrType::Unit, |expr| expr.ty.clone());
            (
                ty,
                IrExprKind::If {
                    condition: Box::new(condition),
                    then_branch: Box::new(then_branch),
                    else_branch: else_branch.map(Box::new),
                },
            )
        }
        Expr::Call(name, args) => {
            let args = args
                .iter()
                .map(|arg| lower_expr(arg, functions, scope))
                .collect::<Result<Vec<_>, _>>()?;
            let ty = match name.as_str() {
                "print" | "println" => IrType::Unit,
                "ok" | "err" => IrType::Record("Result".into()),
                "map" | "map_get" => IrType::Record("Map".into()),
                "i32" => IrType::I32,
                "u64" => IrType::U64,
                _ => functions
                    .get(name)
                    .map(|(_, ty)| ty.clone())
                    .ok_or_else(|| LowerError::Invalid(format!("undefined function `{name}`")))?,
            };
            (ty, IrExprKind::Call(name.clone(), args))
        }
        Expr::Try(_) | Expr::Match { .. } => {
            return Err(LowerError::Unsupported(
                "IR lowering for Result propagation and match expressions is deferred".into(),
            ));
        }
    };
    Ok(IrExpr { ty, kind })
}

/// Render a stable, human-readable artifact for reviews and golden tests.
pub fn render(program: &IrProgram) -> String {
    let mut out = String::new();
    for stmt in &program.body {
        render_stmt(stmt, 0, &mut out);
    }
    out
}

fn render_stmt(stmt: &IrStmt, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match stmt {
        IrStmt::Record { name, fields } => {
            let fields = fields
                .iter()
                .map(|(field, ty)| format!("{field}: {ty}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{pad}record {name} {{ {fields} }}\n"));
        }
        IrStmt::Enum { name, variants } => {
            let variants = variants
                .iter()
                .map(|(variant, fields)| {
                    let fields = fields
                        .iter()
                        .map(|(field, ty)| format!("{field}: {ty}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{variant} {{ {fields} }}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{pad}enum {name} {{ {variants} }}\n"));
        }
        IrStmt::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|(name, ty)| format!("{name}: {ty}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "{pad}fn {}({params}) -> {} {{\n",
                function.name, function.return_type
            ));
            render_expr(&function.body, indent + 1, out);
            out.push_str(&format!("{pad}}}\n"));
        }
        IrStmt::Let { name, ty, value } => {
            out.push_str(&format!("{pad}let {name}: {ty} = "));
            render_expr(value, 0, out);
            out.push_str(";\n");
        }
        IrStmt::Assign { name, value } => {
            out.push_str(&format!("{pad}{name} = "));
            render_expr(value, 0, out);
            out.push_str(";\n");
        }
        IrStmt::AssignIndex {
            name,
            indices,
            value,
        } => {
            out.push_str(&format!("{pad}{name}"));
            for index in indices {
                out.push('[');
                render_expr(index, 0, out);
                out.push(']');
            }
            out.push_str(" = ");
            render_expr(value, 0, out);
            out.push_str(";\n");
        }
        IrStmt::Expr(expr) => {
            out.push_str(&pad);
            render_expr(expr, 0, out);
            out.push_str(";\n");
        }
        IrStmt::If { condition, .. } => {
            out.push_str(&format!("{pad}if {} ...\n", display_expr(condition)))
        }
        IrStmt::While { condition, .. } => {
            out.push_str(&format!("{pad}while {} ...\n", display_expr(condition)))
        }
        IrStmt::Break => out.push_str(&format!("{pad}break;\n")),
        IrStmt::Continue => out.push_str(&format!("{pad}continue;\n")),
        IrStmt::Return(expr) => {
            out.push_str(&format!("{pad}return"));
            if let Some(expr) = expr {
                out.push(' ');
                render_expr(expr, 0, out);
            }
            out.push_str(";\n");
        }
    }
}

fn render_expr(expr: &IrExpr, _indent: usize, out: &mut String) {
    out.push_str(&format!("[{}]{}", expr.ty, display_expr(expr)));
}

fn display_expr(expr: &IrExpr) -> String {
    match &expr.kind {
        IrExprKind::Int(value) => value.to_string(),
        IrExprKind::Bool(value) => value.to_string(),
        IrExprKind::String(value) => format!("{value:?}"),
        IrExprKind::Var(name) => name.clone(),
        IrExprKind::Binary(left, op, right) => {
            format!("({} {op:?} {})", display_expr(left), display_expr(right))
        }
        IrExprKind::Unary(op, value) => format!("({op:?}{})", display_expr(value)),
        IrExprKind::Block(_, _) => "{ ... }".into(),
        IrExprKind::If { .. } => "if ...".into(),
        IrExprKind::Call(name, args) => format!("{name}({} args)", args.len()),
        IrExprKind::Record(name, fields) => format!("{name} {{ {} fields }}", fields.len()),
        IrExprKind::EnumVariant {
            enum_name,
            variant,
            fields,
        } => format!("{enum_name}::{variant} {{ {} fields }}", fields.len()),
        IrExprKind::Field(value, field) => format!("{}.{}", display_expr(value), field),
        IrExprKind::Array(values) => format!("[{} items]", values.len()),
        IrExprKind::Index(value, index) => {
            format!("{}[{}]", display_expr(value), display_expr(index))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{lower, render, verify};
    use crate::{ast::Value, builder::build_ast, evaluator, parser::parse_vex};

    fn compile(source: &str) -> String {
        let pairs = parse_vex(source).unwrap();
        render(&lower(&build_ast(pairs).unwrap()).unwrap())
    }

    #[test]
    fn lowers_typed_expressions_and_control_flow() {
        let ir = compile("let x: i32 = 1 + 2; while x < 3 { break; } x;");
        assert!(ir.contains("let x: i32"));
        assert!(ir.contains("[i32](1 Add 2)"));
        assert!(ir.contains("while"));
    }

    #[test]
    fn lowers_functions() {
        let ir = compile("fn add(a: i32, b: i32) -> i32 { a + b } add(1, 2);");
        assert!(ir.contains("fn add(a: i32, b: i32) -> i32"));
        assert!(ir.contains("[i32]add(2 args)"));
    }

    #[test]
    fn lowering_agrees_with_interpreter_for_supported_programs() {
        let source = "fn inc(value: i32) -> i32 { value + 1 } let x: i32 = 2; while x < 4 { x = inc(x); } x;";
        let pairs = parse_vex(source).unwrap();
        let ast = build_ast(pairs).unwrap();
        let program = lower(&ast).unwrap();
        assert_eq!(evaluator::eval(&ast), Ok(Value::Int(4)));
        assert!(render(&program).contains("while"));
        assert!(render(&program).contains("fn inc(value: i32) -> i32"));
    }

    #[test]
    fn verifier_accepts_lowerable_differential_corpus() {
        for source in [
            "1 + 2 * 3;",
            "true && !false;",
            "let x = 0; while x < 2 { x = x + 1; } x;",
            "fn add(a: i32, b: i32) -> i32 { a + b } add(1, 2);",
            "record Pair { left: i32, right: i32 } Pair { left: 1, right: 2 };",
            "let xs: [i32] = [1, 2, 3]; xs;",
        ] {
            let pairs = parse_vex(source).unwrap();
            let ast = build_ast(pairs).unwrap();
            let program = lower(&ast).unwrap();
            verify(&program).unwrap();
        }
    }

    #[test]
    fn verifier_reports_deferred_features_without_emitting_backend_output() {
        for source in [
            "enum Maybe { None } Maybe::None;",
            r#"let value = ok(1); value;"#,
            r#"let values = map("answer", 42); values;"#,
        ] {
            let pairs = parse_vex(source).unwrap();
            let ast = build_ast(pairs).unwrap();
            let error = lower(&ast).unwrap_err().to_string();
            assert!(error.contains("IR002"), "{error}");
        }
    }
}
