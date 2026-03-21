use std::fmt;

use crate::{BoolExpr, Env, Expr, Stmt, Stmts};

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
        expr: BoolExpr,
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

/// SSA-style variable identifier: (name, version)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SsaVar {
    pub name: String,
    pub version: usize,
}

impl SsaVar {
    pub fn new(name: impl Into<String>, version: usize) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }
}

impl From<String> for SsaVar {
    fn from(name: String) -> Self {
        Self { name, version: 0 }
    }
}

impl From<&str> for SsaVar {
    fn from(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: 0,
        }
    }
}

impl fmt::Display for SsaVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// State for concolic execution
#[derive(Debug, Clone)]
pub struct ConcolicState {
    /// Concrete values for variables
    pub env: Env,
    /// Collected path constraints (with the branch direction taken)
    ///
    /// These are conditions from if-then-else branches encountered during execution.
    /// Oracle failures are not stored here; they are returned directly in ExploreResult.
    pub path_constraints: Vec<(BoolExpr, bool)>,
    /// Let binding constraints in SSA form: ((name, version), expr)
    ///
    /// When a `let name = expr` statement is executed, the expr is transformed
    /// to use versioned variable names, and a new version is assigned.
    /// For example: `let y = x + 1; let y = y + 1` becomes:
    /// - ((y, 0), x + 1)
    /// - ((y, 1), (y, 0) + 1)
    pub let_constraints: Vec<(SsaVar, Expr)>,
    /// Current version for each variable name
    versions: std::collections::HashMap<String, usize>,
}

impl ConcolicState {
    pub fn new(env: Env) -> Self {
        Self {
            env,
            path_constraints: Vec::new(),
            let_constraints: Vec::new(),
            versions: std::collections::HashMap::new(),
        }
    }

    /// Allocate a new version for a variable and return it
    fn next_version(&mut self, name: &str) -> usize {
        let version = self.versions.entry(name.to_string()).or_insert(0);
        let current = *version;
        *version += 1;
        current
    }

    /// Convert expression to SSA form, replacing let-defined variables with their SSA names
    pub fn to_ssa_expr(&self, expr: &Expr) -> Expr {
        match expr {
            Expr::Lit(n) => Expr::Lit(*n),
            Expr::Var(name) => {
                // If this variable was defined by let, use SSA name
                // Otherwise keep original name (input variable)
                if let Some(&version) = self.versions.get(name) {
                    // Use version - 1 because versions points to the next version
                    Expr::Var(SsaVar::new(name, version - 1).to_string())
                } else {
                    Expr::Var(name.clone())
                }
            }
            Expr::Add(l, r) => {
                Expr::Add(Box::new(self.to_ssa_expr(l)), Box::new(self.to_ssa_expr(r)))
            }
            Expr::Sub(l, r) => {
                Expr::Sub(Box::new(self.to_ssa_expr(l)), Box::new(self.to_ssa_expr(r)))
            }
            Expr::If(cond, then_, else_) => Expr::If(
                Box::new(self.to_ssa_bool_expr(cond)),
                Box::new(self.to_ssa_expr(then_)),
                Box::new(self.to_ssa_expr(else_)),
            ),
        }
    }

    /// Convert boolean expression to SSA form
    pub fn to_ssa_bool_expr(&self, expr: &BoolExpr) -> BoolExpr {
        match expr {
            BoolExpr::Lit(b) => BoolExpr::Lit(*b),
            BoolExpr::Le(l, r) => {
                BoolExpr::Le(Box::new(self.to_ssa_expr(l)), Box::new(self.to_ssa_expr(r)))
            }
            BoolExpr::Ge(l, r) => {
                BoolExpr::Ge(Box::new(self.to_ssa_expr(l)), Box::new(self.to_ssa_expr(r)))
            }
            BoolExpr::Eq(l, r) => {
                BoolExpr::Eq(Box::new(self.to_ssa_expr(l)), Box::new(self.to_ssa_expr(r)))
            }
        }
    }

    /// Evaluate an integer expression
    pub fn eval(&mut self, expr: &Expr) -> Result<i64, OracleFailure> {
        match expr {
            Expr::Lit(n) => Ok(*n),
            Expr::Var(name) => self
                .env
                .get(name)
                .copied()
                .ok_or_else(|| OracleFailure::UndefinedVariable { name: name.clone() }),
            Expr::Add(l, r) => Ok(self.eval(l)? + self.eval(r)?),
            Expr::Sub(l, r) => Ok(self.eval(l)? - self.eval(r)?),
            Expr::If(cond, then_, else_) => {
                // eval_bool records the constraint
                if self.eval_bool(cond)? {
                    self.eval(then_)
                } else {
                    self.eval(else_)
                }
            }
        }
    }

    /// Evaluate a boolean expression and record it as a path constraint
    ///
    /// Used for branch conditions (if-then-else). The condition is recorded
    /// in path_constraints for path exploration.
    pub fn eval_bool(&mut self, expr: &BoolExpr) -> Result<bool, OracleFailure> {
        let result = self.eval_assert(expr)?;
        if !matches!(expr, BoolExpr::Lit(_)) {
            self.path_constraints.push((expr.clone(), result));
        }
        Ok(result)
    }

    /// Evaluate an assertion (property) without recording it as a path constraint
    ///
    /// The assertion expression itself is not recorded to path_constraints,
    /// but any internal branch conditions (from if-then-else in subexpressions)
    /// are still recorded via eval().
    pub fn eval_assert(&mut self, expr: &BoolExpr) -> Result<bool, OracleFailure> {
        match expr {
            BoolExpr::Lit(b) => Ok(*b),
            BoolExpr::Le(l, r) => Ok(self.eval(l)? <= self.eval(r)?),
            BoolExpr::Ge(l, r) => Ok(self.eval(l)? >= self.eval(r)?),
            BoolExpr::Eq(l, r) => Ok(self.eval(l)? == self.eval(r)?),
        }
    }

    /// Execute a single statement, returning Err(OracleFailure) if an assertion fails
    pub fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), OracleFailure> {
        match stmt {
            Stmt::Assert { expr } => {
                if self.eval_assert(expr)? {
                    Ok(())
                } else {
                    Err(OracleFailure::AssertionFailed { expr: expr.clone() })
                }
            }
            Stmt::Let { name, expr } => {
                // Convert expr to SSA form before recording (must be done before next_version)
                let ssa_expr = self.to_ssa_expr(expr);
                // Evaluate the expression and bind to the environment
                let value = self.eval(expr)?;
                self.env.insert(name.clone(), value);
                // Allocate new version for this variable
                let version = self.next_version(name);
                // Record the constraint for the solver (name@version == ssa_expr)
                self.let_constraints
                    .push((SsaVar::new(name.clone(), version), ssa_expr));
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
    fn format_expr(&self, expr: &Expr) -> String {
        let val = expr.eval(&self.env);
        match expr {
            Expr::Lit(n) => format!("{}", n),
            _ => format!("{} [={}]", expr, val),
        }
    }

    /// Format a boolean expression with concrete values
    fn format_bool_expr(&self, expr: &BoolExpr) -> String {
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
        insta::assert_snapshot!(state.eval(&expr).unwrap(), @"6");
        insta::assert_snapshot!(state, @r###"
        Env: x = 5
        Path constraints:
        "###);
    }

    #[test]
    fn eval_if_then_branch() {
        let expr = parse_expr("if x <= 10 then x + 1 else 0").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        insta::assert_snapshot!(state.eval(&expr).unwrap(), @"6");
        insta::assert_snapshot!(state, @r###"
        Env: x = 5
        Path constraints:
          x [=5] <= 10 : true
        "###);
    }

    #[test]
    fn eval_if_else_branch() {
        let expr = parse_expr("if x <= 10 then x + 1 else 0").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 15)]));
        insta::assert_snapshot!(state.eval(&expr).unwrap(), @"0");
        insta::assert_snapshot!(state, @r###"
        Env: x = 15
        Path constraints:
          x [=15] <= 10 : false
        "###);
    }

    #[test]
    fn eval_bool_with_nested_if() {
        // (if x <= 5 then x else 10) <= 7
        // When x = 3: takes then branch, result is 3 <= 7 = true
        let cond = parse_bool_expr("(if x <= 5 then x else 10) <= 7").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 3)]));
        insta::assert_snapshot!(state.eval_bool(&cond).unwrap(), @"true");
        insta::assert_snapshot!(state, @r###"
        Env: x = 3
        Path constraints:
          x [=3] <= 5 : true
          ite(x <= 5, x, 10) [=3] <= 7 : true
        "###);
    }

    #[test]
    fn eval_bool_with_nested_if_else() {
        // (if x <= 5 then x else 10) <= 7
        // When x = 8: takes else branch, result is 10 <= 7 = false
        let cond = parse_bool_expr("(if x <= 5 then x else 10) <= 7").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 8)]));
        insta::assert_snapshot!(state.eval_bool(&cond).unwrap(), @"false");
        insta::assert_snapshot!(state, @r###"
        Env: x = 8
        Path constraints:
          x [=8] <= 5 : false
          ite(x <= 5, x, 10) [=10] <= 7 : false
        "###);
    }

    #[test]
    fn nested_if_in_condition() {
        // if (if x <= 5 then x + 5 else x - 5) <= 3 then 1 else 0
        let expr = parse_expr("if (if x <= 5 then x + 5 else x - 5) <= 3 then 1 else 0").unwrap();

        // x = 2: inner = 2 + 5 = 7, 7 <= 3 is false, result = 0
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 2)]));
        insta::assert_snapshot!(state.eval(&expr).unwrap(), @"0");
        insta::assert_snapshot!(state, @r###"
        Env: x = 2
        Path constraints:
          x [=2] <= 5 : true
          ite(x <= 5, x + 5, x - 5) [=7] <= 3 : false
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
        insta::assert_snapshot!(state.eval(&expr).unwrap(), @"0");
        insta::assert_snapshot!(state, @r###"
        Env: x = 2, y = -1
        Path constraints:
          x [=2] <= 5 : true
          y [=-1] <= 0 : true
          ite(x <= 5, ite(y <= 0, x + 5, x + 6), x - 5) [=7] <= 3 : false
        "###);
    }

    #[test]
    fn display_state() {
        let expr = parse_expr("if x + 1 <= 5 then (if y <= x + 2 then y else 0) else -1").unwrap();

        let mut state =
            ConcolicState::new(HashMap::from([("x".to_string(), 3), ("y".to_string(), 4)]));
        state.eval(&expr).unwrap();

        insta::assert_snapshot!(state, @r###"
        Env: x = 3, y = 4
        Path constraints:
          x + 1 [=4] <= 5 : true
          y [=4] <= x + 2 [=5] : true
        "###);
    }

    #[test]
    fn eval_assert_does_not_record() {
        // eval_assert should evaluate without recording to path_constraints
        let property = parse_bool_expr("x <= 10").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));

        let result = state.eval_assert(&property).unwrap();

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
          y@0 = x + 1
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
          y@0 = ite(x >= 1, x, x + 1)
        Path constraints:
          x [=5] >= 1 : true
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
          y@0 = x + 1
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
    fn display_state_with_let() {
        let stmts = parse_stmts("let y = if x >= 1 then x else x + 1").unwrap();
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        state.exec_stmts(&stmts).unwrap();

        insta::assert_snapshot!(state, @r###"
        Env: x = 5, y = 5
        Let constraints:
          y@0 = ite(x >= 1, x, x + 1)
        Path constraints:
          x [=5] >= 1 : true
        "###);
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
          y@0 = x + 1
          y@1 = y@0 + 1
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
          x@0 = x + 1
          x@1 = x@0 + 1
        Path constraints:
        "###);
    }
}
