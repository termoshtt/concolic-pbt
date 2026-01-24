use std::collections::HashMap;
use std::fmt;

use crate::{BoolExpr, Expr};

/// Environment mapping variable names to concrete values
pub type Env = HashMap<String, i64>;

/// State for concolic execution
#[derive(Debug, Clone)]
pub struct ConcolicState {
    /// Concrete values for variables
    pub env: Env,
    /// Collected path constraints (with the branch direction taken)
    pub constraints: Vec<(BoolExpr, bool)>,
}

impl ConcolicState {
    pub fn new(env: Env) -> Self {
        Self {
            env,
            constraints: Vec::new(),
        }
    }

    /// Evaluate an integer expression
    pub fn eval(&mut self, expr: &Expr) -> i64 {
        match expr {
            Expr::Lit(n) => *n,
            Expr::Var(name) => self.env[name],
            Expr::Add(l, r) => self.eval(l) + self.eval(r),
            Expr::If(cond, then_, else_) => {
                let cond_val = self.eval_bool(cond);
                // Record the constraint with the direction taken
                self.constraints.push((*cond.clone(), cond_val));
                if cond_val {
                    self.eval(then_)
                } else {
                    self.eval(else_)
                }
            }
        }
    }

    /// Evaluate a boolean expression (also records constraints from nested Exprs)
    pub fn eval_bool(&mut self, expr: &BoolExpr) -> bool {
        match expr {
            BoolExpr::Lit(b) => *b,
            BoolExpr::Le(l, r) => self.eval(l) <= self.eval(r),
            BoolExpr::Ge(l, r) => self.eval(l) >= self.eval(r),
            BoolExpr::Eq(l, r) => self.eval(l) == self.eval(r),
        }
    }

    /// Evaluate without recording constraints (for display purposes)
    fn eval_pure(&self, expr: &Expr) -> i64 {
        match expr {
            Expr::Lit(n) => *n,
            Expr::Var(name) => self.env[name],
            Expr::Add(l, r) => self.eval_pure(l) + self.eval_pure(r),
            Expr::If(cond, then_, else_) => {
                if self.eval_bool_pure(cond) {
                    self.eval_pure(then_)
                } else {
                    self.eval_pure(else_)
                }
            }
        }
    }

    fn eval_bool_pure(&self, expr: &BoolExpr) -> bool {
        match expr {
            BoolExpr::Lit(b) => *b,
            BoolExpr::Le(l, r) => self.eval_pure(l) <= self.eval_pure(r),
            BoolExpr::Ge(l, r) => self.eval_pure(l) >= self.eval_pure(r),
            BoolExpr::Eq(l, r) => self.eval_pure(l) == self.eval_pure(r),
        }
    }

    /// Format an expression with its concrete value: "x + 1 [=4]"
    fn format_expr(&self, expr: &Expr) -> String {
        let val = self.eval_pure(expr);
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

        // Constraints
        writeln!(f, "Constraints:")?;
        for (expr, taken) in &self.constraints {
            writeln!(f, "  {} : {}", self.format_bool_expr(expr), taken)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmp;

    #[test]
    fn display_state() {
        let x = Expr::var("x");
        let y = Expr::var("y");
        let expr = Expr::if_(
            cmp!(x.clone() + Expr::lit(1), <=, Expr::lit(5)),
            Expr::if_(
                cmp!(y.clone(), <=, x + Expr::lit(2)),
                y,
                Expr::lit(0),
            ),
            Expr::lit(-1),
        );

        let mut state = ConcolicState::new(HashMap::from([
            ("x".to_string(), 3),
            ("y".to_string(), 4),
        ]));
        state.eval(&expr);

        insta::assert_snapshot!(state, @r#"
        Env: x = 3, y = 4
        Constraints:
          x + 1 [=4] <= 5 : true
          y [=4] <= x + 2 [=5] : true
        "#);
    }
}
