use std::fmt;

use crate::{Ast, BoolExpr, Env, Expr, SsaVar, Stmt, Stmts, SymIfBranches, Symbolic};

/// Oracle failure types
///
/// These represent failures detected during evaluation (assertion failures, NaN, Inf, etc.)
/// that are separate from path constraints used for exploration.
///
/// - Path constraints: conditions from if-then-else branches (used for path exploration)
/// - Oracle failures: property violations detected during evaluation (used for bug detection)
#[derive(Debug, Clone, PartialEq)]
pub enum OracleFailure {
    /// Assertion failure (property evaluated to false)
    ///
    /// Currently, `find_counterexample(f)` implicitly treats `f: BoolExpr` as `assert(f)`.
    /// When `Stmt` is added to the language, this will correspond to explicit assert statements.
    AssertionFailed {
        /// The boolean expression that was asserted and evaluated to false
        expr: BoolExpr<Ast>,
    },
    /// Undefined variable reference
    UndefinedVariable {
        /// The name of the undefined variable
        name: String,
    },
    // Future extensions:
    // NaN { tensor: String },
    // Inf { tensor: String },
}

/// Result of executing a sequence of statements (immutable)
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    /// Final environment after execution (includes let-bound variables)
    pub env: Env,
    /// Collected path constraints (with the branch direction taken)
    pub path_constraints: Vec<(BoolExpr<Symbolic>, bool)>,
    /// Let binding constraints in SSA form
    pub let_constraints: Vec<(SsaVar, Expr<Symbolic>)>,
    /// Assertions that passed during execution (in SSA form)
    pub passed_asserts: Vec<BoolExpr<Symbolic>>,
    /// Execution result (Ok if all assertions passed, Err if one failed)
    pub result: Result<(), OracleFailure>,
}

/// Execute statements and return the execution trace
pub fn exec(stmts: &Stmts, env: Env) -> ExecutionTrace {
    let mut state = ConcolicState::new(env);
    let result = state.exec_stmts(stmts);
    ExecutionTrace {
        env: state.env,
        path_constraints: state.path_constraints,
        let_constraints: state.let_constraints,
        passed_asserts: state.passed_asserts,
        result,
    }
}

/// Internal state for concolic execution (one statement at a time)
#[derive(Debug, Clone)]
pub(crate) struct ConcolicState {
    /// Concrete values for variables
    pub env: Env,
    /// Collected path constraints (with the branch direction taken)
    ///
    /// These are conditions from if-then-else branches encountered during execution.
    /// Oracle failures are not stored here; they are returned directly in ExploreResult.
    pub path_constraints: Vec<(BoolExpr<Symbolic>, bool)>,
    /// Let binding constraints in SSA form: ((name, version), expr)
    ///
    /// When a `let name = expr` statement is executed, the expr is transformed
    /// to use versioned variable names, and a new version is assigned.
    /// For example: `let y = x + 1; let y = y + 1` becomes:
    /// - ((y, 0), x + 1)
    /// - ((y, 1), (y, 0) + 1)
    pub let_constraints: Vec<(SsaVar, Expr<Symbolic>)>,
    /// Assertions that passed during execution (in SSA form)
    pub passed_asserts: Vec<BoolExpr<Symbolic>>,
    /// Current version for each variable name
    versions: std::collections::HashMap<String, usize>,
}

impl ConcolicState {
    pub fn new(env: Env) -> Self {
        // Initialize version counts to 0 for input variables.
        // count=0 means "Input" (not yet defined by let), count>=1 means "Defined(count)"
        let versions = env.keys().map(|k| (k.clone(), 0)).collect();
        Self {
            env,
            path_constraints: Vec::new(),
            let_constraints: Vec::new(),
            passed_asserts: Vec::new(),
            versions,
        }
    }

    /// Allocate a new version for a variable and return the SsaVar
    fn next_ssa_var(&mut self, name: &str) -> SsaVar {
        let count = self.versions.entry(name.to_string()).or_insert(0);
        *count += 1;
        SsaVar::defined(name, *count)
    }

    /// Get the current SSA variable for a name
    fn current_ssa_var(&self, name: &str) -> SsaVar {
        match self.versions.get(name) {
            Some(&count) if count > 0 => SsaVar::defined(name, count),
            _ => SsaVar::input(name),
        }
    }

    /// Evaluate an integer expression, returning both the value and its SSA form
    ///
    /// For If expressions, the non-evaluated branch is kept as Expr<Ast> rather than
    /// being converted to SSA form. This correctly represents that only one branch
    /// was actually executed during this evaluation.
    pub fn eval(&mut self, expr: &Expr<Ast>) -> Result<(i64, Expr<Symbolic>), OracleFailure> {
        match expr {
            Expr::Lit(n) => Ok((*n, Expr::Lit(*n))),
            Expr::Var(name) => {
                let val = self
                    .env
                    .get(name)
                    .copied()
                    .ok_or_else(|| OracleFailure::UndefinedVariable { name: name.clone() })?;
                let ssa_var = self.current_ssa_var(name);
                Ok((val, Expr::Var(ssa_var)))
            }
            Expr::Add(l, r) => {
                let (l_val, l_sym) = self.eval(l)?;
                let (r_val, r_sym) = self.eval(r)?;
                Ok((l_val + r_val, Expr::Add(Box::new(l_sym), Box::new(r_sym))))
            }
            Expr::Sub(l, r) => {
                let (l_val, l_sym) = self.eval(l)?;
                let (r_val, r_sym) = self.eval(r)?;
                Ok((l_val - r_val, Expr::Sub(Box::new(l_sym), Box::new(r_sym))))
            }
            Expr::If(cond, branches) => {
                // eval_bool records the constraint and returns SSA form
                let (cond_val, cond_sym) = self.eval_bool(cond)?;
                if cond_val {
                    let (then_val, then_sym) = self.eval(&branches.then_)?;
                    // Keep else branch as Ast (not evaluated)
                    Ok((
                        then_val,
                        Expr::If(
                            Box::new(cond_sym),
                            SymIfBranches::ThenTaken {
                                then_: Box::new(then_sym),
                                else_: branches.else_.clone(),
                            },
                        ),
                    ))
                } else {
                    let (else_val, else_sym) = self.eval(&branches.else_)?;
                    // Keep then branch as Ast (not evaluated)
                    Ok((
                        else_val,
                        Expr::If(
                            Box::new(cond_sym),
                            SymIfBranches::ElseTaken {
                                then_: branches.then_.clone(),
                                else_: Box::new(else_sym),
                            },
                        ),
                    ))
                }
            }
        }
    }

    /// Evaluate a boolean expression and record it as a path constraint
    ///
    /// Used for branch conditions (if-then-else). The condition is recorded
    /// in path_constraints for path exploration.
    /// Returns both the boolean result and the SSA form.
    pub fn eval_bool(
        &mut self,
        expr: &BoolExpr<Ast>,
    ) -> Result<(bool, BoolExpr<Symbolic>), OracleFailure> {
        let (result, ssa_expr) = self.eval_assert(expr)?;
        if !matches!(expr, BoolExpr::Lit(_)) {
            self.path_constraints.push((ssa_expr.clone(), result));
        }
        Ok((result, ssa_expr))
    }

    /// Evaluate an assertion (property) without recording it as a path constraint
    ///
    /// The assertion expression itself is not recorded to path_constraints,
    /// but any internal branch conditions (from if-then-else in subexpressions)
    /// are still recorded via eval().
    /// Returns both the boolean result and the SSA form.
    pub fn eval_assert(
        &mut self,
        expr: &BoolExpr<Ast>,
    ) -> Result<(bool, BoolExpr<Symbolic>), OracleFailure> {
        match expr {
            BoolExpr::Lit(b) => Ok((*b, BoolExpr::Lit(*b))),
            BoolExpr::Le(l, r) => {
                let (l_val, l_sym) = self.eval(l)?;
                let (r_val, r_sym) = self.eval(r)?;
                Ok((
                    l_val <= r_val,
                    BoolExpr::Le(Box::new(l_sym), Box::new(r_sym)),
                ))
            }
            BoolExpr::Ge(l, r) => {
                let (l_val, l_sym) = self.eval(l)?;
                let (r_val, r_sym) = self.eval(r)?;
                Ok((
                    l_val >= r_val,
                    BoolExpr::Ge(Box::new(l_sym), Box::new(r_sym)),
                ))
            }
            BoolExpr::Eq(l, r) => {
                let (l_val, l_sym) = self.eval(l)?;
                let (r_val, r_sym) = self.eval(r)?;
                Ok((
                    l_val == r_val,
                    BoolExpr::Eq(Box::new(l_sym), Box::new(r_sym)),
                ))
            }
        }
    }

    /// Execute a single statement, returning Err(OracleFailure) if an assertion fails
    pub fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), OracleFailure> {
        match stmt {
            Stmt::Assert { expr } => {
                let (result, ssa_expr) = self.eval_assert(expr)?;
                if result {
                    self.passed_asserts.push(ssa_expr);
                    Ok(())
                } else {
                    Err(OracleFailure::AssertionFailed { expr: expr.clone() })
                }
            }
            Stmt::Let { name, expr } => {
                // Evaluate the expression (also returns SSA form)
                let (value, ssa_expr) = self.eval(expr)?;
                self.env.insert(name.clone(), value);
                // Allocate new version for this variable and record constraint
                let ssa_var = self.next_ssa_var(name);
                self.let_constraints.push((ssa_var, ssa_expr));
                Ok(())
            }
        }
    }

    /// Execute a sequence of statements, returning Err(OracleFailure) if any assertion fails
    pub fn exec_stmts(&mut self, stmts: &Stmts) -> Result<(), OracleFailure> {
        for stmt in &stmts.0 {
            self.exec_stmt(stmt)?;
        }
        Ok(())
    }

    /// Format an expression with its concrete value: "x + 1 [=4]"
    fn format_expr(&self, expr: &Expr<Symbolic>) -> String {
        match expr {
            Expr::Lit(n) => format!("{}", n),
            Expr::Var(ssa_var) => {
                // Look up the concrete value using the base name.
                // This is only called after successful execution, so all variables
                // in Expr<Symbolic> must be in env (otherwise eval would have
                // returned UndefinedVariable error).
                let val = self
                    .env
                    .get(&ssa_var.name)
                    .copied()
                    .expect("BUG: variable not in env after successful execution");
                format!("{} [={}]", ssa_var, val)
            }
            Expr::Add(l, r) | Expr::Sub(l, r) => {
                // For binary ops, format sub-expressions recursively
                let left = self.format_expr(l);
                let right = self.format_expr(r);
                let op = if matches!(expr, Expr::Add(_, _)) {
                    "+"
                } else {
                    "-"
                };
                format!("{} {} {}", left, op, right)
            }
            Expr::If(_, _) => format!("{}", expr),
        }
    }

    /// Format a boolean expression with concrete values
    fn format_bool_expr(&self, expr: &BoolExpr<Symbolic>) -> String {
        match expr {
            BoolExpr::Lit(b) => format!("{}", b),
            BoolExpr::Le(l, r) => {
                format!("{} <= {}", self.format_expr(l), self.format_expr(r))
            }
            BoolExpr::Ge(l, r) => {
                format!("{} >= {}", self.format_expr(l), self.format_expr(r))
            }
            BoolExpr::Eq(l, r) => {
                format!("{} == {}", self.format_expr(l), self.format_expr(r))
            }
        }
    }
}

impl fmt::Display for ConcolicState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Env
        write!(f, "Env: ")?;
        let mut vars: Vec<_> = self.env.iter().collect();
        vars.sort_by_key(|(k, _)| *k);
        for (i, (name, val)) in vars.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{} = {}", name, val)?;
        }
        writeln!(f)?;

        // Let constraints
        if !self.let_constraints.is_empty() {
            writeln!(f, "Let constraints:")?;
            for (ssa_var, expr) in &self.let_constraints {
                writeln!(f, "  {} = {}", ssa_var, expr)?;
            }
        }

        // Path constraints
        writeln!(f, "Path constraints:")?;
        for (expr, taken) in &self.path_constraints {
            writeln!(f, "  {} : {}", self.format_bool_expr(expr), taken)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{parse_bool_expr, parse_expr, parse_stmts};

    #[test]
    fn eval_simple() {
        let expr = parse_expr("x + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        let (val, _sym) = state.eval(&expr).unwrap();
        insta::assert_snapshot!(val, @"6");
        insta::assert_snapshot!(state, @r###"
        Env: x = 5
        Path constraints:
        "###);
    }

    #[test]
    fn eval_if_then_branch() {
        let expr = parse_expr("if x <= 10 then x + 1 else 0").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        let (val, _sym) = state.eval(&expr).unwrap();
        insta::assert_snapshot!(val, @"6");
        insta::assert_snapshot!(state, @r###"
        Env: x = 5
        Path constraints:
          x@0 [=5] <= 10 : true
        "###);
    }

    #[test]
    fn eval_if_else_branch() {
        let expr = parse_expr("if x <= 10 then x + 1 else 0").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 15)]));
        let (val, _sym) = state.eval(&expr).unwrap();
        insta::assert_snapshot!(val, @"0");
        insta::assert_snapshot!(state, @r###"
        Env: x = 15
        Path constraints:
          x@0 [=15] <= 10 : false
        "###);
    }

    #[test]
    fn eval_bool_with_nested_if() {
        // (if x <= 5 then x else 10) <= 7
        // When x = 3: takes then branch, result is 3 <= 7 = true
        let cond = parse_bool_expr("(if x <= 5 then x else 10) <= 7").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 3)]));
        let (val, _sym) = state.eval_bool(&cond).unwrap();
        insta::assert_snapshot!(val, @"true");
        insta::assert_snapshot!(state, @r###"
        Env: x = 3
        Path constraints:
          x@0 [=3] <= 5 : true
          ite(x@0 <= 5, x@0, 10) <= 7 : true
        "###);
    }

    #[test]
    fn eval_bool_with_nested_if_else() {
        // (if x <= 5 then x else 10) <= 7
        // When x = 8: takes else branch, result is 10 <= 7 = false
        let cond = parse_bool_expr("(if x <= 5 then x else 10) <= 7").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 8)]));
        let (val, _sym) = state.eval_bool(&cond).unwrap();
        insta::assert_snapshot!(val, @"false");
        insta::assert_snapshot!(state, @r###"
        Env: x = 8
        Path constraints:
          x@0 [=8] <= 5 : false
          ite(x@0 <= 5, x, 10) <= 7 : false
        "###);
    }

    #[test]
    fn nested_if_in_condition() {
        // if (if x <= 5 then x + 5 else x - 5) <= 3 then 1 else 0
        let expr = parse_expr("if (if x <= 5 then x + 5 else x - 5) <= 3 then 1 else 0").unwrap();

        // x = 2: inner = 2 + 5 = 7, 7 <= 3 is false, result = 0
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 2)]));
        let (val, _sym) = state.eval(&expr).unwrap();
        insta::assert_snapshot!(val, @"0");
        insta::assert_snapshot!(state, @r###"
        Env: x = 2
        Path constraints:
          x@0 [=2] <= 5 : true
          ite(x@0 <= 5, x@0 + 5, x - 5) <= 3 : false
        "###);
    }

    #[test]
    fn deeply_nested_if() {
        // if (if x <= 5 then (if y <= 0 then x+5 else x+6) else x-5) <= 3 then 1 else 0
        let expr = parse_expr(
            "if (if x <= 5 then (if y <= 0 then x + 5 else x + 6) else x - 5) <= 3 then 1 else 0",
        )
        .unwrap();

        // x = 2, y = -1: inner_inner = 2+5 = 7, inner = 7, 7 <= 3 is false, result = 0
        let mut state =
            ConcolicState::new(HashMap::from([("x".to_string(), 2), ("y".to_string(), -1)]));
        let (val, _sym) = state.eval(&expr).unwrap();
        insta::assert_snapshot!(val, @"0");
        insta::assert_snapshot!(state, @r###"
        Env: x = 2, y = -1
        Path constraints:
          x@0 [=2] <= 5 : true
          y@0 [=-1] <= 0 : true
          ite(x@0 <= 5, ite(y@0 <= 0, x@0 + 5, x + 6), x - 5) <= 3 : false
        "###);
    }

    #[test]
    fn display_state() {
        let expr = parse_expr("if x + 1 <= 5 then (if y <= x + 2 then y else 0) else -1").unwrap();

        let mut state =
            ConcolicState::new(HashMap::from([("x".to_string(), 3), ("y".to_string(), 4)]));
        let _ = state.eval(&expr).unwrap();

        insta::assert_snapshot!(state, @r###"
        Env: x = 3, y = 4
        Path constraints:
          x@0 [=3] + 1 <= 5 : true
          y@0 [=4] <= x@0 [=3] + 2 : true
        "###);
    }

    #[test]
    fn eval_assert_does_not_record() {
        // eval_assert should evaluate without recording to path_constraints
        let property = parse_bool_expr("x <= 10").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));

        let (result, _sym) = state.eval_assert(&property).unwrap();

        assert!(result);
        assert!(
            state.path_constraints.is_empty(),
            "eval_assert should not record path constraints"
        );
    }

    #[test]
    fn exec_assert_pass() {
        let stmts = parse_stmts("assert(x <= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        assert!(state.exec_stmts(&stmts).is_ok());
    }

    #[test]
    fn exec_assert_fail() {
        let stmts = parse_stmts("assert(x <= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 15)]));
        assert!(matches!(
            state.exec_stmts(&stmts),
            Err(OracleFailure::AssertionFailed { .. })
        ));
    }

    #[test]
    fn exec_seq() {
        // Multiple asserts
        let stmts = parse_stmts("assert(x >= 0); assert(x <= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        assert!(state.exec_stmts(&stmts).is_ok());
    }

    #[test]
    fn exec_seq_early_fail() {
        // First assert fails, second is not reached
        let stmts = parse_stmts("assert(x <= 0); assert(x >= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        assert!(matches!(
            state.exec_stmts(&stmts),
            Err(OracleFailure::AssertionFailed { .. })
        ));
    }

    #[test]
    fn exec_let_simple() {
        // let y = x + 1
        let stmts = parse_stmts("let y = x + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        state.exec_stmts(&stmts).unwrap();
        insta::assert_snapshot!(state, @r###"
        Env: x = 5, y = 6
        Let constraints:
          y@1 = x@0 + 1
        Path constraints:
        "###);
    }

    #[test]
    fn exec_let_with_if() {
        // let y = if x >= 1 then x else x + 1
        // When x = 5: y = 5, path constraint: x >= 1 : true
        let stmts = parse_stmts("let y = if x >= 1 then x else x + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        state.exec_stmts(&stmts).unwrap();
        insta::assert_snapshot!(state, @r###"
        Env: x = 5, y = 5
        Let constraints:
          y@1 = ite(x@0 >= 1, x@0, x + 1)
        Path constraints:
          x@0 [=5] >= 1 : true
        "###);
    }

    #[test]
    fn exec_let_then_assert() {
        // let y = x + 1; assert(y <= 10)
        let stmts = parse_stmts("let y = x + 1; assert(y <= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        state.exec_stmts(&stmts).unwrap();
        insta::assert_snapshot!(state, @r###"
        Env: x = 5, y = 6
        Let constraints:
          y@1 = x@0 + 1
        Path constraints:
        "###);
    }

    #[test]
    fn exec_let_then_assert_fail() {
        // let y = x + 1; assert(y <= 10)
        // When x = 15: y = 16, assertion fails
        let stmts = parse_stmts("let y = x + 1; assert(y <= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 15)]));
        assert!(matches!(
            state.exec_stmts(&stmts),
            Err(OracleFailure::AssertionFailed { .. })
        ));
    }

    #[test]
    fn undefined_variable_error() {
        // Reference undefined variable 'y'
        let expr = parse_expr("x + y").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));

        let result = state.eval(&expr);
        assert!(matches!(
            result,
            Err(OracleFailure::UndefinedVariable { name }) if name == "y"
        ));
    }

    #[test]
    fn undefined_variable_in_assert() {
        let stmts = parse_stmts("assert(y <= 10)").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));

        let result = state.exec_stmts(&stmts);
        assert!(matches!(
            result,
            Err(OracleFailure::UndefinedVariable { name }) if name == "y"
        ));
    }

    #[test]
    fn undefined_variable_in_let() {
        // let y = z + 1 where z is undefined
        let stmts = parse_stmts("let y = z + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));

        let result = state.exec_stmts(&stmts);
        assert!(matches!(
            result,
            Err(OracleFailure::UndefinedVariable { name }) if name == "z"
        ));
    }

    #[test]
    fn shadowing_let() {
        // let y = x + 1; let y = y + 1
        // x = 5 -> y = 6 -> y = 7
        let stmts = parse_stmts("let y = x + 1; let y = y + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        state.exec_stmts(&stmts).unwrap();
        insta::assert_snapshot!(state, @r###"
        Env: x = 5, y = 7
        Let constraints:
          y@1 = x@0 + 1
          y@2 = y@1 + 1
        Path constraints:
        "###);
    }

    #[test]
    fn shadowing_input_variable() {
        // let x = x + 1; let x = x + 1
        // x = 5 -> x = 6 -> x = 7
        let stmts = parse_stmts("let x = x + 1; let x = x + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        state.exec_stmts(&stmts).unwrap();
        insta::assert_snapshot!(state, @r###"
        Env: x = 7
        Let constraints:
          x@1 = x@0 + 1
          x@2 = x@1 + 1
        Path constraints:
        "###);
    }
}
