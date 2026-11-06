//! Backend contracts and deliberately non-native emission.
//!
//! This module is the target boundary.  A backend must accept only the
//! `FIRST_TARGET` subset; it must never infer a runtime representation for
//! values outside that contract.

use crate::ir::{self, IrExpr, IrExprKind, IrFunction, IrProgram, IrStmt, IrType};

/// The first native-target contract.  It is intentionally target-independent:
/// a future QBE or machine backend can implement this contract without
/// changing language semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetContract {
    pub name: &'static str,
    pub entry_point: &'static str,
    pub calling_convention: &'static str,
    pub overflow: &'static str,
    pub division: &'static str,
    pub control_flow: &'static str,
    pub runtime_boundary: &'static str,
}

pub const FIRST_TARGET: TargetContract = TargetContract {
    name: "vex-scalar-v1",
    entry_point: "vex_main",
    calling_convention: "vex_main() -> i32; internal scalar functions pass i32/bool left-to-right and return one scalar",
    overflow: "i32 add/sub/mul/neg trap on overflow; no wrapping is permitted",
    division: "i32 division traps on zero and on MIN / -1",
    control_flow: "if/while are structured; break exits the innermost loop, continue jumps to its condition",
    runtime_boundary: "no I/O, allocation, or external calls; traps are runtime-owned and entry has no arguments",
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    pub code: &'static str,
    pub backend: &'static str,
    pub message: String,
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]: {}", self.code, self.backend, self.message)
    }
}

impl std::error::Error for BackendError {}

/// Validate that an IR program is representable by the first target.
pub fn validate_first_target(program: &IrProgram) -> Result<(), BackendError> {
    ir::verify(program).map_err(|error| BackendError {
        code: error.code,
        backend: FIRST_TARGET.name,
        message: error.to_string(),
    })?;
    for (index, stmt) in program.body.iter().enumerate() {
        validate_stmt(stmt, &format!("body[{index}]"), &program.functions)?;
    }
    Ok(())
}

fn validate_type(ty: &IrType, path: &str) -> Result<(), BackendError> {
    match ty {
        IrType::I32 | IrType::Bool | IrType::Unit => Ok(()),
        _ => Err(unsupported(
            path,
            format!("type `{ty}` is outside the scalar target"),
        )),
    }
}

fn validate_stmt(stmt: &IrStmt, path: &str, functions: &[IrFunction]) -> Result<(), BackendError> {
    match stmt {
        IrStmt::Function(function) => {
            validate_type(&function.return_type, &format!("{path}.return"))?;
            for (name, ty) in &function.params {
                validate_type(ty, &format!("{path}.param.{name}"))?;
            }
            validate_expr(&function.body, &format!("{path}.body"), functions)
        }
        IrStmt::Let { ty, value, .. } => {
            validate_type(ty, &format!("{path}.type"))?;
            validate_expr(value, &format!("{path}.value"), functions)
        }
        IrStmt::Assign { value, .. } => validate_expr(value, &format!("{path}.value"), functions),
        IrStmt::Expr(expr) => validate_expr(expr, &format!("{path}.expr"), functions),
        IrStmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            validate_expr(condition, &format!("{path}.condition"), functions)?;
            validate_expr(then_branch, &format!("{path}.then"), functions)?;
            if let Some(branch) = else_branch {
                validate_expr(branch, &format!("{path}.else"), functions)?;
            }
            Ok(())
        }
        IrStmt::While { condition, body } => {
            validate_expr(condition, &format!("{path}.condition"), functions)?;
            validate_expr(body, &format!("{path}.body"), functions)
        }
        IrStmt::Break | IrStmt::Continue => Ok(()),
        IrStmt::Return(value) => value
            .as_ref()
            .map(|expr| validate_expr(expr, &format!("{path}.value"), functions))
            .unwrap_or(Ok(())),
        IrStmt::Record { .. } | IrStmt::Enum { .. } | IrStmt::AssignIndex { .. } => {
            Err(unsupported(
                path,
                "aggregate values and indexed mutation are not in the first target",
            ))
        }
    }
}

fn validate_expr(expr: &IrExpr, path: &str, functions: &[IrFunction]) -> Result<(), BackendError> {
    validate_type(&expr.ty, &format!("{path}.type"))?;
    match &expr.kind {
        IrExprKind::Int(_) | IrExprKind::Bool(_) | IrExprKind::Var(_) => Ok(()),
        IrExprKind::Binary(left, _, right) => {
            validate_expr(left, &format!("{path}.left"), functions)?;
            validate_expr(right, &format!("{path}.right"), functions)
        }
        IrExprKind::Unary(_, value) => validate_expr(value, &format!("{path}.value"), functions),
        IrExprKind::Block(stmts, tail) => {
            for (index, stmt) in stmts.iter().enumerate() {
                validate_stmt(stmt, &format!("{path}.stmt[{index}]"), functions)?;
            }
            tail.as_ref()
                .map(|expr| validate_expr(expr, &format!("{path}.tail"), functions))
                .unwrap_or(Ok(()))
        }
        IrExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            validate_expr(condition, &format!("{path}.condition"), functions)?;
            validate_expr(then_branch, &format!("{path}.then"), functions)?;
            else_branch
                .as_ref()
                .map(|expr| validate_expr(expr, &format!("{path}.else"), functions))
                .unwrap_or(Ok(()))
        }
        IrExprKind::Call(name, args) => {
            if functions.iter().any(|f| &f.name == name) {
                for (i, arg) in args.iter().enumerate() {
                    validate_expr(arg, &format!("{path}.arg[{i}]"), functions)?;
                }
                Ok(())
            } else if name == "i32" {
                Err(unsupported(
                    path,
                    "integer conversions are outside the first target",
                ))
            } else {
                Err(unsupported(
                    path,
                    format!("call `{name}` has no runtime boundary"),
                ))
            }
        }
        IrExprKind::String(_)
        | IrExprKind::Record(_, _)
        | IrExprKind::Field(_, _)
        | IrExprKind::Array(_)
        | IrExprKind::EnumVariant { .. }
        | IrExprKind::Index(_, _) => Err(unsupported(
            path,
            "non-scalar expression is outside the first target",
        )),
    }
}

fn unsupported(path: &str, message: impl Into<String>) -> BackendError {
    BackendError {
        code: "BE002",
        backend: FIRST_TARGET.name,
        message: format!("at {path}: {}", message.into()),
    }
}

pub trait Backend {
    type Error: std::error::Error;

    fn name(&self) -> &'static str;
    fn emit(&self, program: &IrProgram) -> Result<String, Self::Error>;
}

/// A verified, target-independent artifact.  It is useful for acceptance and
/// golden tests, but is not QBE, assembly, or an executable.
pub struct TextBackend;

impl Backend for TextBackend {
    type Error = BackendError;

    fn name(&self) -> &'static str {
        "vex-text-v1"
    }

    fn emit(&self, program: &IrProgram) -> Result<String, Self::Error> {
        validate_first_target(program)?;
        Ok(format!(
            "target {}\nentry {}\ncalling-convention {}\noverflow {}\ndivision {}\ncontrol-flow {}\nruntime {}\n---\n{}",
            FIRST_TARGET.name,
            FIRST_TARGET.entry_point,
            FIRST_TARGET.calling_convention,
            FIRST_TARGET.overflow,
            FIRST_TARGET.division,
            FIRST_TARGET.control_flow,
            FIRST_TARGET.runtime_boundary,
            ir::render(program)
        ))
    }
}

/// QBE native intermediate language generator for the first scalar target.
pub struct QbeBackend;

impl Backend for QbeBackend {
    type Error = BackendError;

    fn name(&self) -> &'static str {
        "qbe"
    }

    fn emit(&self, program: &IrProgram) -> Result<String, Self::Error> {
        validate_first_target(program)?;
        emit_qbe(program)
    }
}

fn emit_qbe(program: &IrProgram) -> Result<String, BackendError> {
    use std::fmt::Write as _;
    let mut out = String::new();
    writeln!(&mut out, "# QBE intermediate language emitted by Vex").unwrap();
    writeln!(&mut out, "# Target: {}\n", FIRST_TARGET.name).unwrap();

    for function in &program.functions {
        let func_il = emit_qbe_function(function)?;
        out.push_str(&func_il);
        out.push('\n');
    }

    let main_il = emit_qbe_main(&program.body)?;
    out.push_str(&main_il);

    Ok(out)
}

#[derive(Clone, Copy)]
enum TrapKind {
    Overflow,
    DivisionByZero,
    DivisionOverflow,
}

impl TrapKind {
    fn exit_code(self) -> i32 {
        match self {
            Self::Overflow => 101,
            Self::DivisionByZero => 102,
            Self::DivisionOverflow => 103,
        }
    }
}

struct QbeEmitter {
    allocations: Vec<String>,
    body: String,
    temp_count: usize,
    label_count: usize,
    slot_count: usize,
    env: Vec<std::collections::HashMap<String, String>>,
    loop_stack: Vec<(String, String)>,
    current_block_terminated: bool,
}

impl QbeEmitter {
    fn new() -> Self {
        Self {
            allocations: Vec::new(),
            body: String::new(),
            temp_count: 0,
            label_count: 0,
            slot_count: 0,
            env: vec![std::collections::HashMap::new()],
            loop_stack: Vec::new(),
            current_block_terminated: false,
        }
    }

    fn push_scope(&mut self) {
        self.env.push(std::collections::HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop();
    }

    fn fresh_temp(&mut self) -> String {
        let t = format!("%t{}", self.temp_count);
        self.temp_count += 1;
        t
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let l = format!("@{}_{}", prefix, self.label_count);
        self.label_count += 1;
        l
    }

    fn alloc_slot(&mut self, name: &str) -> String {
        let slot = format!("%v_{}_{}", name, self.slot_count);
        self.slot_count += 1;
        self.allocations.push(format!("    {slot} =l alloc4 4"));
        if let Some(scope) = self.env.last_mut() {
            scope.insert(name.to_string(), slot.clone());
        }
        slot
    }

    fn fresh_slot(&mut self) -> String {
        let slot = format!("%s_{}", self.slot_count);
        self.slot_count += 1;
        self.allocations.push(format!("    {slot} =l alloc4 4"));
        slot
    }

    fn get_slot(&self, name: &str) -> Option<String> {
        for scope in self.env.iter().rev() {
            if let Some(slot) = scope.get(name) {
                return Some(slot.clone());
            }
        }
        None
    }

    fn emit_label(&mut self, label: &str) {
        if !self.current_block_terminated {
            self.body.push_str(&format!("    jmp {label}\n"));
        }
        self.body.push_str(&format!("{label}\n"));
        self.current_block_terminated = false;
    }

    fn emit_instruction(&mut self, instr: &str) {
        if self.current_block_terminated {
            let unreachable_lbl = self.fresh_label("unreachable");
            self.emit_label(&unreachable_lbl);
        }
        self.body.push_str(&format!("    {instr}\n"));
    }

    fn emit_jump(&mut self, target: &str) {
        self.emit_instruction(&format!("jmp {target}"));
        self.current_block_terminated = true;
    }

    fn emit_branch(&mut self, cond: &str, if_true: &str, if_false: &str) {
        self.emit_instruction(&format!("jnz {cond}, {if_true}, {if_false}"));
        self.current_block_terminated = true;
    }

    fn emit_ret(&mut self, val: Option<&str>) {
        if let Some(v) = val {
            self.emit_instruction(&format!("ret {v}"));
        } else {
            self.emit_instruction("ret 0");
        }
        self.current_block_terminated = true;
    }

    fn emit_hlt(&mut self) {
        self.emit_instruction("hlt");
        self.current_block_terminated = true;
    }

    fn emit_trap_if(&mut self, cond: &str, kind: TrapKind) {
        let trap_lbl = self.fresh_label("trap");
        let ok_lbl = self.fresh_label("trap_ok");
        self.emit_branch(cond, &trap_lbl, &ok_lbl);
        self.emit_label(&trap_lbl);
        self.emit_instruction(&format!("call $exit(w {})", kind.exit_code()));
        self.emit_hlt();
        self.emit_label(&ok_lbl);
    }

    fn emit_checked_add(&mut self, left: &str, right: &str) -> String {
        let result = self.fresh_temp();
        self.emit_instruction(&format!("{result} =w add {left}, {right}"));
        let left_sign = self.fresh_temp();
        self.emit_instruction(&format!("{left_sign} =w xor {result}, {left}"));
        let right_sign = self.fresh_temp();
        self.emit_instruction(&format!("{right_sign} =w xor {result}, {right}"));
        let same_sign_overflow_bits = self.fresh_temp();
        self.emit_instruction(&format!(
            "{same_sign_overflow_bits} =w and {left_sign}, {right_sign}"
        ));
        let overflow = self.fresh_temp();
        self.emit_instruction(&format!("{overflow} =w csltw {same_sign_overflow_bits}, 0"));
        self.emit_trap_if(&overflow, TrapKind::Overflow);
        result
    }

    fn emit_checked_sub(&mut self, left: &str, right: &str) -> String {
        let result = self.fresh_temp();
        self.emit_instruction(&format!("{result} =w sub {left}, {right}"));
        let operand_signs_differ = self.fresh_temp();
        self.emit_instruction(&format!("{operand_signs_differ} =w xor {left}, {right}"));
        let result_sign_differs = self.fresh_temp();
        self.emit_instruction(&format!("{result_sign_differs} =w xor {left}, {result}"));
        let overflow_bits = self.fresh_temp();
        self.emit_instruction(&format!(
            "{overflow_bits} =w and {operand_signs_differ}, {result_sign_differs}"
        ));
        let overflow = self.fresh_temp();
        self.emit_instruction(&format!("{overflow} =w csltw {overflow_bits}, 0"));
        self.emit_trap_if(&overflow, TrapKind::Overflow);
        result
    }

    fn emit_checked_mul(&mut self, left: &str, right: &str) -> String {
        let left_wide = self.fresh_temp();
        self.emit_instruction(&format!("{left_wide} =l extsw {left}"));
        let right_wide = self.fresh_temp();
        self.emit_instruction(&format!("{right_wide} =l extsw {right}"));
        let wide = self.fresh_temp();
        self.emit_instruction(&format!("{wide} =l mul {left_wide}, {right_wide}"));
        let result = self.fresh_temp();
        self.emit_instruction(&format!("{result} =w copy {wide}"));
        let roundtrip = self.fresh_temp();
        self.emit_instruction(&format!("{roundtrip} =l extsw {result}"));
        let overflow = self.fresh_temp();
        self.emit_instruction(&format!("{overflow} =w cnel {wide}, {roundtrip}"));
        self.emit_trap_if(&overflow, TrapKind::Overflow);
        result
    }

    fn emit_checked_div(&mut self, left: &str, right: &str) -> String {
        let zero = self.fresh_temp();
        self.emit_instruction(&format!("{zero} =w ceqw {right}, 0"));
        let min_left = self.fresh_temp();
        self.emit_instruction(&format!("{min_left} =w ceqw {left}, -2147483648"));
        let neg_one_right = self.fresh_temp();
        self.emit_instruction(&format!("{neg_one_right} =w ceqw {right}, -1"));
        let div_overflow = self.fresh_temp();
        self.emit_instruction(&format!(
            "{div_overflow} =w and {min_left}, {neg_one_right}"
        ));
        self.emit_trap_if(&zero, TrapKind::DivisionByZero);
        self.emit_trap_if(&div_overflow, TrapKind::DivisionOverflow);
        let result = self.fresh_temp();
        self.emit_instruction(&format!("{result} =w div {left}, {right}"));
        result
    }

    fn emit_checked_neg(&mut self, value: &str) -> String {
        let overflow = self.fresh_temp();
        self.emit_instruction(&format!("{overflow} =w ceqw {value}, -2147483648"));
        self.emit_trap_if(&overflow, TrapKind::Overflow);
        let result = self.fresh_temp();
        self.emit_instruction(&format!("{result} =w neg {value}"));
        result
    }

    fn emit_expr(&mut self, expr: &IrExpr) -> String {
        use crate::ast::Op;
        match &expr.kind {
            IrExprKind::Int(val) => {
                let temp = self.fresh_temp();
                self.emit_instruction(&format!("{temp} =w copy {val}"));
                temp
            }
            IrExprKind::Bool(val) => {
                let temp = self.fresh_temp();
                let b = if *val { 1 } else { 0 };
                self.emit_instruction(&format!("{temp} =w copy {b}"));
                temp
            }
            IrExprKind::Var(name) => {
                let slot = self
                    .get_slot(name)
                    .unwrap_or_else(|| panic!("ICE: undefined variable `{name}` in QBE emitter"));
                let temp = self.fresh_temp();
                self.emit_instruction(&format!("{temp} =w loadw {slot}"));
                temp
            }
            IrExprKind::Binary(left, op, right) => match op {
                Op::Add => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    self.emit_checked_add(&l, &r)
                }
                Op::Sub => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    self.emit_checked_sub(&l, &r)
                }
                Op::Mul => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    self.emit_checked_mul(&l, &r)
                }
                Op::Div => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    self.emit_checked_div(&l, &r)
                }
                Op::Eq => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w ceqw {l}, {r}"));
                    temp
                }
                Op::Lt => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w csltw {l}, {r}"));
                    temp
                }
                Op::Gt => {
                    let l = self.emit_expr(left);
                    let r = self.emit_expr(right);
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w csgtw {l}, {r}"));
                    temp
                }
                Op::And => {
                    let l = self.emit_expr(left);
                    let res_slot = self.fresh_slot();
                    self.emit_instruction(&format!("storew 0, {res_slot}"));
                    let rhs_lbl = self.fresh_label("and_rhs");
                    let end_lbl = self.fresh_label("and_end");
                    self.emit_branch(&l, &rhs_lbl, &end_lbl);

                    self.emit_label(&rhs_lbl);
                    let r = self.emit_expr(right);
                    if !self.current_block_terminated {
                        self.emit_instruction(&format!("storew {r}, {res_slot}"));
                        self.emit_jump(&end_lbl);
                    }

                    self.emit_label(&end_lbl);
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w loadw {res_slot}"));
                    temp
                }
                Op::Or => {
                    let l = self.emit_expr(left);
                    let res_slot = self.fresh_slot();
                    self.emit_instruction(&format!("storew 1, {res_slot}"));
                    let rhs_lbl = self.fresh_label("or_rhs");
                    let end_lbl = self.fresh_label("or_end");
                    self.emit_branch(&l, &end_lbl, &rhs_lbl);

                    self.emit_label(&rhs_lbl);
                    let r = self.emit_expr(right);
                    if !self.current_block_terminated {
                        self.emit_instruction(&format!("storew {r}, {res_slot}"));
                        self.emit_jump(&end_lbl);
                    }

                    self.emit_label(&end_lbl);
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w loadw {res_slot}"));
                    temp
                }
                _ => panic!("ICE: unsupported binary operator in QBE emitter: {:?}", op),
            },
            IrExprKind::Unary(op, value) => match op {
                Op::Sub => {
                    let v = self.emit_expr(value);
                    self.emit_checked_neg(&v)
                }
                Op::Not => {
                    let v = self.emit_expr(value);
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w ceqw {v}, 0"));
                    temp
                }
                _ => panic!("ICE: unsupported unary operator in QBE emitter: {:?}", op),
            },
            IrExprKind::Block(stmts, tail) => {
                self.push_scope();
                for stmt in stmts {
                    self.emit_stmt(stmt);
                }
                let res = if let Some(tail_expr) = tail {
                    self.emit_expr(tail_expr)
                } else {
                    let temp = self.fresh_temp();
                    self.emit_instruction(&format!("{temp} =w copy 0"));
                    temp
                };
                self.pop_scope();
                res
            }
            IrExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.emit_expr(condition);
                let res_slot = self.fresh_slot();
                let then_lbl = self.fresh_label("if_then");
                let else_lbl = self.fresh_label("if_else");
                let join_lbl = self.fresh_label("if_join");

                self.emit_branch(&cond, &then_lbl, &else_lbl);

                self.emit_label(&then_lbl);
                let then_val = self.emit_expr(then_branch);
                if !self.current_block_terminated {
                    self.emit_instruction(&format!("storew {then_val}, {res_slot}"));
                    self.emit_jump(&join_lbl);
                }

                self.emit_label(&else_lbl);
                if let Some(else_expr) = else_branch {
                    let else_val = self.emit_expr(else_expr);
                    if !self.current_block_terminated {
                        self.emit_instruction(&format!("storew {else_val}, {res_slot}"));
                        self.emit_jump(&join_lbl);
                    }
                } else if !self.current_block_terminated {
                    self.emit_instruction(&format!("storew 0, {res_slot}"));
                    self.emit_jump(&join_lbl);
                }

                self.emit_label(&join_lbl);
                let temp = self.fresh_temp();
                self.emit_instruction(&format!("{temp} =w loadw {res_slot}"));
                temp
            }
            IrExprKind::Call(name, args) => {
                let mut arg_temps = Vec::new();
                for arg in args {
                    let t = self.emit_expr(arg);
                    arg_temps.push(format!("w {t}"));
                }
                let temp = self.fresh_temp();
                self.emit_instruction(&format!(
                    "{temp} =w call ${}({})",
                    crate::ast::link_name(name),
                    arg_temps.join(", ")
                ));
                temp
            }
            _ => panic!("ICE: non-scalar expression slipped past target validation"),
        }
    }

    fn emit_stmt(&mut self, stmt: &IrStmt) -> Option<String> {
        match stmt {
            IrStmt::Function(_) => None,
            IrStmt::Let { name, value, .. } => {
                let val = self.emit_expr(value);
                let slot = self.alloc_slot(name);
                self.emit_instruction(&format!("storew {val}, {slot}"));
                None
            }
            IrStmt::Assign { name, value } => {
                let val = self.emit_expr(value);
                let slot = self
                    .get_slot(name)
                    .unwrap_or_else(|| panic!("ICE: undefined variable `{name}` in assignment"));
                self.emit_instruction(&format!("storew {val}, {slot}"));
                None
            }
            IrStmt::Expr(expr) => {
                let t = self.emit_expr(expr);
                Some(t)
            }
            IrStmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.emit_expr(condition);
                let then_lbl = self.fresh_label("if_then");
                let else_lbl = self.fresh_label("if_else");
                let join_lbl = self.fresh_label("if_join");

                self.emit_branch(&cond, &then_lbl, &else_lbl);

                self.emit_label(&then_lbl);
                self.emit_expr(then_branch);
                if !self.current_block_terminated {
                    self.emit_jump(&join_lbl);
                }

                self.emit_label(&else_lbl);
                if let Some(else_expr) = else_branch {
                    self.emit_expr(else_expr);
                }
                if !self.current_block_terminated {
                    self.emit_jump(&join_lbl);
                }

                self.emit_label(&join_lbl);
                None
            }
            IrStmt::While { condition, body } => {
                let cond_lbl = self.fresh_label("while_cond");
                let body_lbl = self.fresh_label("while_body");
                let end_lbl = self.fresh_label("while_end");

                self.emit_jump(&cond_lbl);

                self.emit_label(&cond_lbl);
                let cond = self.emit_expr(condition);
                self.emit_branch(&cond, &body_lbl, &end_lbl);

                self.loop_stack.push((cond_lbl.clone(), end_lbl.clone()));
                self.emit_label(&body_lbl);
                self.emit_expr(body);
                if !self.current_block_terminated {
                    self.emit_jump(&cond_lbl);
                }
                self.loop_stack.pop();

                self.emit_label(&end_lbl);
                None
            }
            IrStmt::Break => {
                let (_, end_lbl) = self
                    .loop_stack
                    .last()
                    .expect("ICE: break outside loop slipped past validation")
                    .clone();
                self.emit_jump(&end_lbl);
                None
            }
            IrStmt::Continue => {
                let (cond_lbl, _) = self
                    .loop_stack
                    .last()
                    .expect("ICE: continue outside loop slipped past validation")
                    .clone();
                self.emit_jump(&cond_lbl);
                None
            }
            IrStmt::Return(value) => {
                let val_temp = value.as_ref().map(|expr| self.emit_expr(expr));
                self.emit_ret(val_temp.as_deref());
                None
            }
            IrStmt::Record { .. } | IrStmt::Enum { .. } | IrStmt::AssignIndex { .. } => {
                panic!("ICE: aggregate statement slipped past target validation");
            }
        }
    }
}

fn emit_qbe_function(function: &IrFunction) -> Result<String, BackendError> {
    let mut emitter = QbeEmitter::new();
    let params: Vec<String> = function
        .params
        .iter()
        .map(|(name, _)| format!("w %p_{name}"))
        .collect();

    for (name, _) in &function.params {
        let slot = emitter.alloc_slot(name);
        emitter.emit_instruction(&format!("storew %p_{name}, {slot}"));
    }

    let ret_val = emitter.emit_expr(&function.body);
    if !emitter.current_block_terminated {
        emitter.emit_ret(Some(&ret_val));
    }

    let mut out = format!(
        "function w ${}({}) {{\n@start\n",
        crate::ast::link_name(&function.name),
        params.join(", ")
    );
    for alloc in &emitter.allocations {
        out.push_str(alloc);
        out.push('\n');
    }
    out.push_str(&emitter.body);
    out.push_str("}\n");
    Ok(out)
}

fn emit_qbe_main(body: &[IrStmt]) -> Result<String, BackendError> {
    let mut emitter = QbeEmitter::new();
    let mut last_val = None;

    for stmt in body {
        if let Some(v) = emitter.emit_stmt(stmt) {
            last_val = Some(v);
        }
    }

    if !emitter.current_block_terminated {
        emitter.emit_ret(last_val.as_deref());
    }

    let mut out = format!(
        "export function w ${}() {{\n@start\n",
        FIRST_TARGET.entry_point
    );
    for alloc in &emitter.allocations {
        out.push_str(alloc);
        out.push('\n');
    }
    out.push_str(&emitter.body);
    out.push_str("}\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Backend, FIRST_TARGET, TextBackend, validate_first_target};
    use crate::{builder::build_ast, ir, parser::parse_vex};

    fn lower(source: &str) -> ir::IrProgram {
        let pairs = parse_vex(source).unwrap();
        ir::lower(&build_ast(pairs).unwrap()).unwrap()
    }

    fn qbe_toolchain_available() -> bool {
        std::process::Command::new("qbe").arg("-h").output().is_ok()
            && std::process::Command::new("cc")
                .arg("--version")
                .output()
                .is_ok()
    }

    fn compile_and_run_qbe(source: &str, index: usize) -> Option<i32> {
        if !qbe_toolchain_available() {
            eprintln!("skipping optional QBE execution test: qbe or cc not found");
            return None;
        }

        let qbe = super::QbeBackend.emit(&lower(source)).unwrap();
        let dir = std::env::temp_dir().join(format!("vex-qbe-test-{}-{index}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ssa = dir.join("program.ssa");
        let asm = dir.join("program.s");
        let driver = dir.join("driver.c");
        let exe = dir.join("program");
        std::fs::write(&ssa, qbe).unwrap();
        std::fs::write(
            &driver,
            "extern int vex_main(void); int main(void) { return vex_main(); }\n",
        )
        .unwrap();

        let assembly = std::process::Command::new("qbe")
            .arg(&ssa)
            .output()
            .expect("qbe should run after availability check");
        assert!(
            assembly.status.success(),
            "qbe rejected generated IL for `{source}`: {}",
            String::from_utf8_lossy(&assembly.stderr)
        );
        std::fs::write(&asm, assembly.stdout).unwrap();

        let cc = std::process::Command::new("cc")
            .arg(&asm)
            .arg(&driver)
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("cc should run after availability check");
        assert!(
            cc.status.success(),
            "cc rejected QBE assembly for `{source}`: {}",
            String::from_utf8_lossy(&cc.stderr)
        );

        let run = std::process::Command::new(&exe).status().unwrap();
        let code = run.code();
        let _ = std::fs::remove_dir_all(&dir);
        code
    }

    #[test]
    fn contract_invariants_are_explicit() {
        assert_eq!(FIRST_TARGET.entry_point, "vex_main");
        assert!(FIRST_TARGET.calling_convention.contains("i32/bool"));
        assert!(FIRST_TARGET.overflow.contains("trap"));
        assert!(FIRST_TARGET.division.contains("zero"));
        assert!(FIRST_TARGET.runtime_boundary.contains("no I/O"));
    }

    #[test]
    fn accepts_scalar_control_flow_and_text_emits_only_after_validation() {
        let program = lower("let x: i32 = 1; while x < 3 { x = x + 1; } x;");
        validate_first_target(&program).unwrap();
        let text = TextBackend.emit(&program).unwrap();
        assert!(text.starts_with("target vex-scalar-v1\nentry vex_main\n"));
        assert!(text.contains("let x: i32"));
    }

    #[test]
    fn rejects_u64_io_and_aggregates() {
        for source in ["let x: u64 = u64(1); x;", "let xs = [1, 2]; xs;"] {
            let program = lower(source);
            assert!(validate_first_target(&program).is_err(), "{source}");
            assert!(TextBackend.emit(&program).is_err(), "{source}");
            assert!(super::QbeBackend.emit(&program).is_err(), "{source}");
        }
    }

    #[test]
    fn qbe_emits_scalar_main_and_functions() {
        let source = "fn add(a: i32, b: i32) -> i32 { a + b } let x = add(2, 3); while x < 10 { x = x + 1; } x;";
        let program = lower(source);
        let qbe = super::QbeBackend.emit(&program).unwrap();
        assert!(qbe.contains("function w $add(w %p_a, w %p_b)"));
        assert!(qbe.contains("export function w $vex_main()"));
        assert!(qbe.contains("alloc4 4"));
        assert!(qbe.contains("call $add"));
        assert!(qbe.contains("@while_cond_"));
        assert!(qbe.contains("ret"));
    }

    #[test]
    fn qbe_emits_nested_control_flow_loop_control_and_early_return() {
        let source = "fn pick(n: i32) -> i32 { if n < 0 { return 7; } n } let x = 0; let total = 0; while x < 6 { x = x + 1; if x == 2 { continue; } if x == 5 { break; } total = total + pick(x); } total;";
        let qbe = super::QbeBackend.emit(&lower(source)).unwrap();
        assert!(qbe.contains("function w $pick(w %p_n)"));
        assert!(qbe.contains("@if_then_"));
        assert!(qbe.contains("@if_else_"));
        assert!(qbe.contains("@while_cond_"));
        assert!(qbe.contains("@while_end_"));
        assert!(qbe.contains("call $pick"));
        assert!(qbe.matches("ret").count() >= 3);
    }

    #[test]
    fn qbe_emits_short_circuit_boolean_branches() {
        let source = "let a = 0; let b = 1; let c = if ((a == 1) && (b == 1)) || (b == 1) { 42 } else { 0 }; c;";
        let qbe = super::QbeBackend.emit(&lower(source)).unwrap();
        assert!(qbe.contains("@and_rhs_"));
        assert!(qbe.contains("@and_end_"));
        assert!(qbe.contains("@or_rhs_"));
        assert!(qbe.contains("@or_end_"));
        assert!(qbe.contains("jnz"));
    }

    #[test]
    fn qbe_emits_runtime_traps_for_i32_errors() {
        for (source, expected_exit) in [
            ("let x = 2147483647; x + 1;", 101),
            ("let x = -2147483647 - 1; -x;", 101),
            ("let x = 50000; x * x;", 101),
            ("let x = 1; let y = 0; x / y;", 102),
            ("let x = -2147483647 - 1; x / -1;", 103),
        ] {
            let qbe = super::QbeBackend.emit(&lower(source)).unwrap();
            assert!(qbe.contains("@trap_"), "missing trap block for {source}");
            assert!(
                qbe.contains(&format!("call $exit(w {expected_exit})")),
                "missing trap exit {expected_exit} for {source}"
            );
            assert!(qbe.contains("hlt"), "missing hlt for {source}");
        }
    }

    #[test]
    fn qbe_trap_exit_codes_match_runtime_failures_when_toolchain_is_available() {
        for (index, (source, expected_exit)) in [
            ("let x = 2147483647; x + 1;", 101),
            ("let x = -2147483647 - 1; x - 1;", 101),
            ("let x = 50000; x * x;", 101),
            ("let x = -2147483647 - 1; -x;", 101),
            ("let x = 1; let y = 0; x / y;", 102),
            ("let x = -2147483647 - 1; x / -1;", 103),
        ]
        .iter()
        .enumerate()
        {
            let Some(code) = compile_and_run_qbe(source, index) else {
                return;
            };
            assert_eq!(code, *expected_exit, "compiled trap code for `{source}`");
        }
    }

    #[test]
    fn qbe_matches_interpreter_for_scalar_arithmetic_when_toolchain_is_available() {
        for (index, (source, expected)) in [
            ("1 + 2 * 3;", 7),
            ("let x = 20; let y = 6; x - y * 2;", 8),
            ("let x = 40; let y = -5; x / -y;", 8),
            ("let x = 2147483647; x - 2147483400;", 247),
            ("let x = -2147483647 - 1; let y = x + 2147483647; -y;", 1),
            ("let x = 46340; let y = x * x; y / 214739560;", 10),
        ]
        .iter()
        .enumerate()
        {
            let Some(code) = compile_and_run_qbe(source, index + 100) else {
                return;
            };
            assert_eq!(code, *expected, "compiled result for `{source}`");
        }
    }

    #[test]
    fn qbe_matches_interpreter_for_scalar_control_flow_when_toolchain_is_available() {
        for (index, (source, expected)) in [
            ("let x = 0; let total = 0; while x < 5 { x = x + 1; total = total + x; } total;", 15),
            ("let x = 0; let total = 0; while x < 5 { x = x + 1; if x == 3 { continue; } total = total + x; } total;", 12),
            ("let x = 0; while x < 10 { x = x + 1; if x == 4 { break; } } x;", 4),
            ("let x = 0; let y = 0; while x < 3 { x = x + 1; let z = 0; while z < 3 { z = z + 1; y = y + x * z; } } y;", 36),
            ("let x = 7; let y = if x < 0 { 1 } else { if x == 7 { 42 } else { 2 } }; y;", 42),
        ]
        .iter()
        .enumerate()
        {
            let Some(code) = compile_and_run_qbe(source, index + 200) else {
                return;
            };
            assert_eq!(code, *expected, "compiled result for `{source}`");
        }
    }

    #[test]
    fn qbe_matches_interpreter_for_scalar_functions_when_toolchain_is_available() {
        for (index, (source, expected)) in [
            ("fn add3(a: i32, b: i32, c: i32) -> i32 { a + b + c } add3(2, 3, 4);", 9),
            ("fn pick(n: i32) -> i32 { if n < 0 { return 7; } n + 1 } pick(-2);", 7),
            ("fn f(n: i32) -> i32 { if n < 2 { return 1; } f(n - 1) + f(n - 2) } f(6);", 13),
            ("fn square(n: i32) -> i32 { n * n } fn sum(a: i32, b: i32) -> i32 { a + b } sum(square(3), square(4));", 25),
        ]
        .iter()
        .enumerate()
        {
            let Some(code) = compile_and_run_qbe(source, index + 300) else {
                return;
            };
            assert_eq!(code, *expected, "compiled result for `{source}`");
        }
    }

    #[test]
    fn qbe_matches_interpreter_for_scalar_booleans_when_toolchain_is_available() {
        for (index, (source, expected)) in [
            (
                "let x = 3; let y = if (x > 2) && (x < 4) { 11 } else { 22 }; y;",
                11,
            ),
            (
                "let x = 0; let y = if (x == 0) || (1 / x == 0) { 33 } else { 44 }; y;",
                33,
            ),
            (
                "let x = 0; let y = if !(x == 0) && (1 / x == 0) { 55 } else { 66 }; y;",
                66,
            ),
            ("let ok = true; let y = if !ok { 1 } else { 77 }; y;", 77),
            (
                "let x = 5; let y = if (x == 5) == true { 88 } else { 99 }; y;",
                88,
            ),
        ]
        .iter()
        .enumerate()
        {
            let Some(code) = compile_and_run_qbe(source, index + 400) else {
                return;
            };
            assert_eq!(code, *expected, "compiled result for `{source}`");
        }
    }
}
