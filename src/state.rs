use std::fmt;

use crate::{BoolExpr, Env, Expr};

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
            Expr::Sub(l, r) => self.eval(l) - self.eval(r),
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
    use std::collections::HashMap;

    use super::*;
    use crate::cmp;

    #[test]
    fn eval_simple() {
        let expr = Expr::var("x") + Expr::lit(1);
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        insta::assert_snapshot!(state.eval(&expr), @"6");
        insta::assert_snapshot!(state, @r#"
        Env: x = 5
        Constraints:
        "#);
    }

    #[test]
    fn eval_if_then_branch() {
        // if x <= 10 then x + 1 else 0
        let x = Expr::var("x");
        let expr = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(10)),
            x + Expr::lit(1),
            Expr::lit(0),
        );

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        insta::assert_snapshot!(state.eval(&expr), @"6");
        insta::assert_snapshot!(state, @r#"
        Env: x = 5
        Constraints:
          x [=5] <= 10 : true
        "#);
    }

    #[test]
    fn eval_if_else_branch() {
        // if x <= 10 then x + 1 else 0
        let x = Expr::var("x");
        let expr = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(10)),
            x + Expr::lit(1),
            Expr::lit(0),
        );

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 15)]));
        insta::assert_snapshot!(state.eval(&expr), @"0");
        insta::assert_snapshot!(state, @r#"
        Env: x = 15
        Constraints:
          x [=15] <= 10 : false
        "#);
    }

    #[test]
    fn eval_bool_with_nested_if() {
        // (if x <= 5 then x else 10) <= 7
        // When x = 3: takes then branch, result is 3 <= 7 = true
        let x = Expr::var("x");
        let inner = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            x,
            Expr::lit(10),
        );
        let cond = cmp!(inner, <=, Expr::lit(7));

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 3)]));
        insta::assert_snapshot!(state.eval_bool(&cond), @"true");
        insta::assert_snapshot!(state, @r#"
        Env: x = 3
        Constraints:
          x [=3] <= 5 : true
        "#);
    }

    #[test]
    fn eval_bool_with_nested_if_else() {
        // (if x <= 5 then x else 10) <= 7
        // When x = 8: takes else branch, result is 10 <= 7 = false
        let x = Expr::var("x");
        let inner = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            x,
            Expr::lit(10),
        );
        let cond = cmp!(inner, <=, Expr::lit(7));

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 8)]));
        insta::assert_snapshot!(state.eval_bool(&cond), @"false");
        insta::assert_snapshot!(state, @r#"
        Env: x = 8
        Constraints:
          x [=8] <= 5 : false
        "#);
    }

    #[test]
    fn nested_if_in_condition() {
        // if (if x <= 5 then x + 5 else x - 5) <= 3 then 1 else 0
        let x = Expr::var("x");
        let inner = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            x.clone() + Expr::lit(5),
            x - Expr::lit(5),
        );
        let expr = Expr::if_(
            cmp!(inner, <=, Expr::lit(3)),
            Expr::lit(1),
            Expr::lit(0),
        );

        // x = 2: inner = 2 + 5 = 7, 7 <= 3 is false, result = 0
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 2)]));
        insta::assert_snapshot!(state.eval(&expr), @"0");
        insta::assert_snapshot!(state, @r#"
        Env: x = 2
        Constraints:
          x [=2] <= 5 : true
          ite(x <= 5, x + 5, x - 5) [=7] <= 3 : false
        "#);
    }

    #[test]
    fn deeply_nested_if() {
        // if (if x <= 5 then (if y <= 0 then x+5 else x+6) else x-5) <= 3 then 1 else 0
        let x = Expr::var("x");
        let y = Expr::var("y");
        let inner_inner = Expr::if_(
            cmp!(y.clone(), <=, Expr::lit(0)),
            x.clone() + Expr::lit(5),
            x.clone() + Expr::lit(6),
        );
        let inner = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            inner_inner,
            x - Expr::lit(5),
        );
        let expr = Expr::if_(
            cmp!(inner, <=, Expr::lit(3)),
            Expr::lit(1),
            Expr::lit(0),
        );

        // x = 2, y = -1: inner_inner = 2+5 = 7, inner = 7, 7 <= 3 is false, result = 0
        let mut state = ConcolicState::new(HashMap::from([
            ("x".to_string(), 2),
            ("y".to_string(), -1),
        ]));
        insta::assert_snapshot!(state.eval(&expr), @"0");
        insta::assert_snapshot!(state, @r#"
        Env: x = 2, y = -1
        Constraints:
          x [=2] <= 5 : true
          y [=-1] <= 0 : true
          ite(x <= 5, ite(y <= 0, x + 5, x + 6), x - 5) [=7] <= 3 : false
        "#);
    }

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
