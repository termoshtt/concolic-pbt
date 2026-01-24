use std::collections::HashMap;

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
}
