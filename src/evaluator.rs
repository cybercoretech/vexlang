use crate::ast::{Expr, Op, Pattern, Stmt, Type, Value};
use std::collections::{BTreeMap, HashMap};
const LOOP_ITERATION_LIMIT: usize = 1_000_000;
const CALL_DEPTH_LIMIT: usize = 256;
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    RuntimeError(String),
    TypeError(String),
}
impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RuntimeError(x) | Self::TypeError(x) => f.write_str(x),
        }
    }
}
impl std::error::Error for EvalError {}
enum Control {
    Value(Value),
    Break,
    Continue,
    Return(Value),
}
type Env = HashMap<String, Value>;
type Functions = HashMap<String, (Vec<(String, Type)>, Type, Expr)>;

#[derive(Clone, Default)]
pub struct Session {
    env: Env,
    functions: Functions,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate only the supplied statements while preserving this session's
    /// bindings and functions between REPL inputs.
    pub fn eval(&mut self, stmts: &[Stmt]) -> Result<Value, EvalError> {
        for stmt in stmts {
            if let Stmt::Function {
                name,
                params,
                return_type,
                body,
            } = stmt
            {
                self.functions.insert(
                    name.clone(),
                    (params.clone(), return_type.clone(), body.clone()),
                );
            }
        }

        let mut last = Value::Int(0);
        for stmt in stmts {
            if matches!(stmt, Stmt::Function { .. }) {
                continue;
            }
            match eval_stmt(stmt, &mut self.env, &self.functions, 0)? {
                Control::Value(value) => last = value,
                Control::Return(_) => {
                    return Err(EvalError::RuntimeError("return outside function".into()));
                }
                Control::Break | Control::Continue => {
                    return Err(EvalError::RuntimeError(
                        "loop control escaped its loop".into(),
                    ));
                }
            }
        }
        Ok(last)
    }
}

pub fn eval(stmts: &[Stmt]) -> Result<Value, EvalError> {
    Session::new().eval(stmts)
}
fn eval_stmt(s: &Stmt, e: &mut Env, f: &Functions, d: usize) -> Result<Control, EvalError> {
    match s {
        Stmt::Function { .. } => Ok(Control::Value(Value::Unit)),
        Stmt::Record { .. } => Ok(Control::Value(Value::Unit)),
        Stmt::Enum { .. } => Ok(Control::Value(Value::Unit)),
        Stmt::Let { name, ty, value } => {
            let v = match eval_expr(value, e, f, d)? {
                Control::Value(v) => v,
                c => return Ok(c),
            };
            validate(name, ty, &v)?;
            e.insert(name.clone(), v.clone());
            Ok(Control::Value(v))
        }
        Stmt::Assign { name, value } => {
            if !e.contains_key(name) {
                return Err(EvalError::RuntimeError(format!(
                    "undefined variable `{name}`"
                )));
            }
            let v = match eval_expr(value, e, f, d)? {
                Control::Value(v) => v,
                c => return Ok(c),
            };
            e.insert(name.clone(), v.clone());
            Ok(Control::Value(v))
        }
        Stmt::AssignIndex {
            name,
            indices,
            value,
        } => {
            let v = value_of(eval_expr(value, e, f, d)?)?;
            let indexes = indices
                .iter()
                .map(|index| {
                    let value = value_of(eval_expr(index, e, f, d)?)?;
                    match value {
                        Value::Int(i) if i >= 0 => usize::try_from(i).map_err(|_| {
                            EvalError::TypeError("array index must be a non-negative i32".into())
                        }),
                        _ => Err(EvalError::TypeError(
                            "array index must be a non-negative i32".into(),
                        )),
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let array = e
                .get_mut(name)
                .ok_or_else(|| EvalError::RuntimeError(format!("undefined variable `{name}`")))?;
            set_index(array, &indexes, v)?;
            Ok(Control::Value(array.clone()))
        }
        Stmt::ExprStmt(x) => eval_expr(x, e, f, d),
        Stmt::Return(x) => Ok(Control::Return(
            x.as_ref()
                .map(|v| value_of(eval_expr(v, e, f, d)?))
                .transpose()?
                .unwrap_or(Value::Unit),
        )),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if as_bool(eval_expr(condition, e, f, d)?)? {
                eval_expr(then_branch, e, f, d)
            } else {
                else_branch
                    .as_ref()
                    .map(|x| eval_expr(x, e, f, d))
                    .unwrap_or(Ok(Control::Value(Value::Unit)))
            }
        }
        Stmt::While { condition, body } => {
            for _ in 0..LOOP_ITERATION_LIMIT {
                if !as_bool(eval_expr(condition, e, f, d)?)? {
                    return Ok(Control::Value(Value::Int(0)));
                }
                match eval_expr(body, e, f, d)? {
                    Control::Break => return Ok(Control::Value(Value::Int(0))),
                    Control::Continue | Control::Value(_) => {}
                    x @ Control::Return(_) => return Ok(x),
                }
            }
            Err(EvalError::RuntimeError(
                "loop iteration limit exceeded".into(),
            ))
        }
        Stmt::Break => Ok(Control::Break),
        Stmt::Continue => Ok(Control::Continue),
    }
}
fn eval_expr(x: &Expr, e: &mut Env, f: &Functions, d: usize) -> Result<Control, EvalError> {
    match x {
        Expr::Int(v) => Ok(Control::Value(Value::Int(*v))),
        Expr::Bool(v) => Ok(Control::Value(Value::Bool(*v))),
        Expr::String(v) => Ok(Control::Value(Value::String(v.clone()))),
        Expr::Array(values) => Ok(Control::Value(Value::Array(
            values
                .iter()
                .map(|v| eval_expr(v, e, f, d).and_then(value_of))
                .collect::<Result<Vec<_>, _>>()?,
        ))),
        Expr::Index(value, index) => {
            let value = value_of(eval_expr(value, e, f, d)?)?;
            let index = value_of(eval_expr(index, e, f, d)?)?;
            let index = match index {
                Value::Int(i) if i >= 0 => usize::try_from(i).map_err(|_| {
                    EvalError::TypeError("array index must be a non-negative i32".into())
                })?,
                _ => {
                    return Err(EvalError::TypeError(
                        "array index must be a non-negative i32".into(),
                    ));
                }
            };
            match value {
                Value::Array(values) => {
                    values
                        .get(index)
                        .cloned()
                        .map(Control::Value)
                        .ok_or_else(|| {
                            EvalError::RuntimeError(format!("array index {index} out of bounds"))
                        })
                }
                _ => Err(EvalError::TypeError("indexing requires an array".into())),
            }
        }
        Expr::Try(value) => match value_of(eval_expr(value, e, f, d)?)? {
            Value::Result { ok: true, value } => Ok(Control::Value(*value)),
            err @ Value::Result { ok: false, .. } => Ok(Control::Return(err)),
            _ => Err(EvalError::TypeError("`?` requires a Result value".into())),
        },
        Expr::Match { value, arms } => {
            let matched = value_of(eval_expr(value, e, f, d)?)?;
            for (pattern, body) in arms {
                let mut local = e.clone();
                let is_match = match (pattern, &matched) {
                    (Pattern::Wildcard, _) => true,
                    (Pattern::Result { ok, binding }, Value::Result { ok: actual, value })
                        if ok == actual =>
                    {
                        if let Some(name) = binding {
                            local.insert(name.clone(), (**value).clone());
                        }
                        true
                    }
                    (
                        Pattern::Enum {
                            enum_name,
                            variant,
                            bindings,
                        },
                        Value::Enum {
                            enum_name: actual_name,
                            variant: actual_variant,
                            fields,
                        },
                    ) if enum_name.as_ref().is_none_or(|name| name == actual_name)
                        && variant == actual_variant =>
                    {
                        for binding in bindings {
                            if let Some(field_value) = fields.get(binding) {
                                local.insert(binding.clone(), field_value.clone());
                            }
                        }
                        true
                    }
                    _ => false,
                };
                if is_match {
                    let result = eval_expr(body, &mut local, f, d)?;
                    merge(e, local, e.keys().cloned().collect());
                    return Ok(result);
                }
            }
            Err(EvalError::RuntimeError("non-exhaustive match".into()))
        }
        Expr::Record(name, fields) => {
            let mut values = BTreeMap::new();
            for (field, expr) in fields {
                values.insert(field.clone(), value_of(eval_expr(expr, e, f, d)?)?);
            }
            Ok(Control::Value(Value::Record(name.clone(), values)))
        }
        Expr::EnumVariant {
            enum_name,
            variant,
            fields,
        } => {
            let mut values = BTreeMap::new();
            for (field, expr) in fields {
                values.insert(field.clone(), value_of(eval_expr(expr, e, f, d)?)?);
            }
            Ok(Control::Value(Value::Enum {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                fields: values,
            }))
        }
        Expr::Field(value, field) => {
            let value = value_of(eval_expr(value, e, f, d)?)?;
            match value {
                Value::Record(_, fields) => fields
                    .get(field)
                    .cloned()
                    .map(Control::Value)
                    .ok_or_else(|| EvalError::RuntimeError(format!("unknown field `{field}`"))),
                _ => Err(EvalError::TypeError(
                    "field access requires a record".into(),
                )),
            }
        }
        Expr::Var(n) => e
            .get(n)
            .cloned()
            .map(Control::Value)
            .ok_or_else(|| EvalError::RuntimeError(format!("undefined variable `{n}`"))),
        Expr::Block(ss, tail) => {
            let names = e.keys().cloned().collect::<Vec<_>>();
            let mut local = e.clone();
            let mut out = Value::Unit;
            for s in ss {
                match eval_stmt(s, &mut local, f, d)? {
                    Control::Value(v) => out = v,
                    c => {
                        merge(e, local, names);
                        return Ok(c);
                    }
                }
            }
            if let Some(t) = tail {
                match eval_expr(t, &mut local, f, d)? {
                    Control::Value(v) => out = v,
                    c => {
                        merge(e, local, names);
                        return Ok(c);
                    }
                }
            }
            merge(e, local, names);
            Ok(Control::Value(out))
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if as_bool(eval_expr(condition, e, f, d)?)? {
                eval_expr(then_branch, e, f, d)
            } else {
                else_branch
                    .as_ref()
                    .map(|x| eval_expr(x, e, f, d))
                    .unwrap_or(Ok(Control::Value(Value::Unit)))
            }
        }
        Expr::Call(n, args) => {
            let vals = args
                .iter()
                .map(|a| value_of(eval_expr(a, e, f, d)?))
                .collect::<Result<Vec<_>, _>>()?;
            if n == "print" || n == "println" {
                if vals.len() != 1 {
                    return Err(EvalError::RuntimeError(format!("{n} expects one argument")));
                }
                print!("{}", display(&vals[0]));
                if n == "println" {
                    println!()
                }
                return Ok(Control::Value(Value::Unit));
            }
            if n == "i32" {
                return Ok(Control::Value(Value::Int(to_i32(&vals[0])? as i64)));
            }
            if n == "u64" {
                return Ok(Control::Value(Value::U64(to_u64(&vals[0])?)));
            }
            if n == "ok" || n == "err" {
                if vals.len() != 1 {
                    return Err(EvalError::RuntimeError(format!("{n} expects one argument")));
                }
                return Ok(Control::Value(Value::Result {
                    ok: n == "ok",
                    value: Box::new(vals.into_iter().next().unwrap()),
                }));
            }
            if n == "map" {
                if vals.len() % 2 != 0 {
                    return Err(EvalError::RuntimeError(
                        "map expects key/value pairs".into(),
                    ));
                }
                let mut map = BTreeMap::new();
                for pair in vals.chunks(2) {
                    let Value::String(key) = &pair[0] else {
                        return Err(EvalError::TypeError("map keys must be strings".into()));
                    };
                    map.insert(key.clone(), pair[1].clone());
                }
                return Ok(Control::Value(Value::Map(map)));
            }
            if n == "map_get" {
                if vals.len() != 2 {
                    return Err(EvalError::RuntimeError(
                        "map_get expects two arguments".into(),
                    ));
                }
                let (Value::Map(map), Value::String(key)) = (&vals[0], &vals[1]) else {
                    return Err(EvalError::TypeError(
                        "map_get expects a map and string key".into(),
                    ));
                };
                return map
                    .get(key)
                    .cloned()
                    .map(Control::Value)
                    .ok_or_else(|| EvalError::RuntimeError(format!("missing map key `{key}`")));
            }
            call(n, vals, f, d)
        }
        Expr::UnaryOp(Op::Not, x) => Ok(Control::Value(Value::Bool(!as_bool(eval_expr(
            x, e, f, d,
        )?)?))),
        Expr::UnaryOp(Op::Sub, x) => match value_of(eval_expr(x, e, f, d)?)? {
            Value::Int(v) => Ok(Control::Value(Value::Int(
                v.checked_neg()
                    .ok_or_else(|| EvalError::RuntimeError("integer overflow".into()))?,
            ))),
            Value::U64(v) => Ok(Control::Value(Value::U64(
                0u64.checked_sub(v)
                    .ok_or_else(|| EvalError::RuntimeError("unsigned underflow".into()))?,
            ))),
            _ => Err(EvalError::TypeError("unary minus requires a number".into())),
        },
        Expr::UnaryOp(_, _) => Err(EvalError::TypeError("unsupported unary operator".into())),
        Expr::BinaryOp(a, op, b) => {
            let l = value_of(eval_expr(a, e, f, d)?)?;
            if (*op == Op::And && l == Value::Bool(false))
                || (*op == Op::Or && l == Value::Bool(true))
            {
                return Ok(Control::Value(l));
            }
            let r = value_of(eval_expr(b, e, f, d)?)?;
            bin(l, op, r)
        }
    }
}
fn call(n: &str, args: Vec<Value>, f: &Functions, d: usize) -> Result<Control, EvalError> {
    if d >= CALL_DEPTH_LIMIT {
        return Err(EvalError::RuntimeError(
            "function call depth limit exceeded".into(),
        ));
    }
    let (params, _, body) = f
        .get(n)
        .ok_or_else(|| EvalError::RuntimeError(format!("undefined function `{n}`")))?;
    let mut e = Env::new();
    for ((name, _), v) in params.iter().zip(args) {
        e.insert(name.clone(), v);
    }
    match eval_expr(body, &mut e, f, d + 1)? {
        Control::Return(v) | Control::Value(v) => Ok(Control::Value(v)),
        Control::Break | Control::Continue => Err(EvalError::RuntimeError(
            "loop control escaped function".into(),
        )),
    }
}
fn merge(e: &mut Env, l: Env, n: Vec<String>) {
    for n in n {
        if let Some(v) = l.get(&n) {
            e.insert(n, v.clone());
        }
    }
}
fn value_of(c: Control) -> Result<Value, EvalError> {
    match c {
        Control::Value(v) | Control::Return(v) => Ok(v),
        Control::Break | Control::Continue => Err(EvalError::RuntimeError(
            "loop control escaped its loop".into(),
        )),
    }
}
fn as_bool(c: Control) -> Result<bool, EvalError> {
    match value_of(c)? {
        Value::Bool(v) => Ok(v),
        _ => Err(EvalError::TypeError("condition must be boolean".into())),
    }
}
fn validate(n: &str, t: &Type, v: &Value) -> Result<(), EvalError> {
    let ok = match t {
        Type::Inferred => true,
        Type::I32 => matches!(v,Value::Int(x)if i32::try_from(*x).is_ok()),
        Type::U64 => match v {
            Value::U64(_) => true,
            Value::Int(x) => *x >= 0,
            _ => false,
        },
        Type::Bool => matches!(v, Value::Bool(_)),
        Type::String => matches!(v, Value::String(_)),
        Type::Unit => matches!(v, Value::Unit),
        Type::Custom(name) => {
            matches!(v, Value::Record(value_name, _) if value_name == name)
                || matches!(v, Value::Enum { enum_name, .. } if enum_name == name)
                || (name == "Result" && matches!(v, Value::Result { .. }))
                || (name == "Map" && matches!(v, Value::Map(_)))
        }
        Type::Array(element) => {
            matches!(v, Value::Array(values) if values.iter().all(|value| validate(n, element, value).is_ok()))
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(EvalError::TypeError(format!(
            "value for `{n}` does not match its annotation"
        )))
    }
}
fn to_i32(v: &Value) -> Result<i32, EvalError> {
    match v {
        Value::Int(x) => i32::try_from(*x)
            .map_err(|_| EvalError::TypeError("cannot convert value to i32".into())),
        Value::U64(x) => i32::try_from(*x)
            .map_err(|_| EvalError::TypeError("cannot convert value to i32".into())),
        _ => Err(EvalError::TypeError("cannot convert value to i32".into())),
    }
}
fn to_u64(v: &Value) -> Result<u64, EvalError> {
    match v {
        Value::Int(x) => u64::try_from(*x)
            .map_err(|_| EvalError::TypeError("cannot convert value to u64".into())),
        Value::U64(x) => Ok(*x),
        _ => Err(EvalError::TypeError("cannot convert value to u64".into())),
    }
}
fn display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Int(x) => x.to_string(),
        Value::U64(x) => x.to_string(),
        Value::Bool(x) => x.to_string(),
        Value::Unit => "()".into(),
        Value::Record(name, fields) => format!("{name}{{{} fields}}", fields.len()),
        Value::Array(values) => format!("[{} items]", values.len()),
        Value::Enum {
            enum_name,
            variant,
            fields,
        } => format!("{enum_name}::{variant}{{{} fields}}", fields.len()),
        Value::Result { ok, value } => {
            format!("{}({})", if *ok { "Ok" } else { "Err" }, display(value))
        }
        Value::Map(map) => format!("Map{{{} entries}}", map.len()),
    }
}
fn set_index(array: &mut Value, indices: &[usize], value: Value) -> Result<(), EvalError> {
    let Some(index) = indices.first() else {
        return Err(EvalError::RuntimeError("missing array index".into()));
    };
    let Value::Array(values) = array else {
        return Err(EvalError::TypeError("indexing requires an array".into()));
    };
    let slot = values
        .get_mut(*index)
        .ok_or_else(|| EvalError::RuntimeError(format!("array index {index} out of bounds")))?;
    if indices.len() == 1 {
        *slot = value;
        Ok(())
    } else {
        set_index(slot, &indices[1..], value)
    }
}
fn bin(a: Value, o: &Op, b: Value) -> Result<Control, EvalError> {
    match (a, o, b) {
        (Value::Int(x), Op::Add, Value::Int(y)) => x
            .checked_add(y)
            .map(|v| Control::Value(Value::Int(v)))
            .ok_or_else(|| EvalError::RuntimeError("integer overflow".into())),
        (Value::Int(x), Op::Sub, Value::Int(y)) => x
            .checked_sub(y)
            .map(|v| Control::Value(Value::Int(v)))
            .ok_or_else(|| EvalError::RuntimeError("integer overflow".into())),
        (Value::Int(x), Op::Mul, Value::Int(y)) => x
            .checked_mul(y)
            .map(|v| Control::Value(Value::Int(v)))
            .ok_or_else(|| EvalError::RuntimeError("integer overflow".into())),
        (Value::Int(_), Op::Div, Value::Int(0)) => {
            Err(EvalError::RuntimeError("division by zero".into()))
        }
        (Value::Int(x), Op::Div, Value::Int(y)) => Ok(Control::Value(Value::Int(x / y))),
        (Value::U64(x), Op::Add, Value::U64(y)) => Ok(Control::Value(Value::U64(
            x.checked_add(y)
                .ok_or_else(|| EvalError::RuntimeError("integer overflow".into()))?,
        ))),
        (Value::U64(x), Op::Sub, Value::U64(y)) => Ok(Control::Value(Value::U64(
            x.checked_sub(y)
                .ok_or_else(|| EvalError::RuntimeError("integer underflow".into()))?,
        ))),
        (Value::U64(x), Op::Mul, Value::U64(y)) => Ok(Control::Value(Value::U64(
            x.checked_mul(y)
                .ok_or_else(|| EvalError::RuntimeError("integer overflow".into()))?,
        ))),
        (Value::U64(x), Op::Div, Value::U64(y)) => x
            .checked_div(y)
            .map(|v| Control::Value(Value::U64(v)))
            .ok_or_else(|| EvalError::RuntimeError("division by zero".into())),
        (Value::Int(x), Op::Eq, Value::Int(y)) => Ok(Control::Value(Value::Bool(x == y))),
        (Value::U64(x), Op::Eq, Value::U64(y)) => Ok(Control::Value(Value::Bool(x == y))),
        (Value::Bool(x), Op::Eq, Value::Bool(y)) => Ok(Control::Value(Value::Bool(x == y))),
        (Value::String(x), Op::Eq, Value::String(y)) => Ok(Control::Value(Value::Bool(x == y))),
        (Value::Int(x), Op::Lt, Value::Int(y)) => Ok(Control::Value(Value::Bool(x < y))),
        (Value::Int(x), Op::Gt, Value::Int(y)) => Ok(Control::Value(Value::Bool(x > y))),
        (Value::U64(x), Op::Lt, Value::U64(y)) => Ok(Control::Value(Value::Bool(x < y))),
        (Value::U64(x), Op::Gt, Value::U64(y)) => Ok(Control::Value(Value::Bool(x > y))),
        (Value::Bool(x), Op::And, Value::Bool(y)) => Ok(Control::Value(Value::Bool(x && y))),
        (Value::Bool(x), Op::Or, Value::Bool(y)) => Ok(Control::Value(Value::Bool(x || y))),
        (_, op, _) => Err(EvalError::TypeError(format!(
            "operator `{op:?}` requires compatible operands"
        ))),
    }
}
