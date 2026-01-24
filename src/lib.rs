/// Abstract Syntax Tree for integer expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer literal
    Lit(i64),
    /// Variable reference by name
    Var(String),
    /// Addition
    Add(Box<Expr>, Box<Expr>),
    /// Conditional expression
    If(Box<BoolExpr>, Box<Expr>, Box<Expr>),
}

/// Boolean expressions
#[derive(Debug, Clone, PartialEq)]
pub enum BoolExpr {
    /// Boolean literal
    Lit(bool),
    /// Less than or equal (<=)
    Le(Box<Expr>, Box<Expr>),
    /// Greater than or equal (>=)
    Ge(Box<Expr>, Box<Expr>),
    /// Equal (==)
    Eq(Box<Expr>, Box<Expr>),
}

use std::collections::HashMap;
use std::ops::Add;

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

impl Expr {
    pub fn lit(n: i64) -> Self {
        Expr::Lit(n)
    }

    pub fn var(name: impl Into<String>) -> Self {
        Expr::Var(name.into())
    }

    pub fn if_(cond: BoolExpr, then_: Expr, else_: Expr) -> Self {
        Expr::If(Box::new(cond), Box::new(then_), Box::new(else_))
    }

    pub fn le(self, rhs: Expr) -> BoolExpr {
        BoolExpr::Le(Box::new(self), Box::new(rhs))
    }

    pub fn ge(self, rhs: Expr) -> BoolExpr {
        BoolExpr::Ge(Box::new(self), Box::new(rhs))
    }

    pub fn eq_(self, rhs: Expr) -> BoolExpr {
        BoolExpr::Eq(Box::new(self), Box::new(rhs))
    }
}

impl BoolExpr {
    pub fn lit(b: bool) -> Self {
        BoolExpr::Lit(b)
    }
}

/// Macro for constructing BoolExpr
///
/// # Examples
/// ```
/// use concolic_pbt::{cmp, Expr};
///
/// let x = Expr::var("x");
/// let cond = cmp!(x, <=, Expr::lit(10));
/// ```
#[macro_export]
macro_rules! cmp {
    ($lhs:expr, <=, $rhs:expr) => {
        $crate::BoolExpr::Le(Box::new($lhs), Box::new($rhs))
    };
    ($lhs:expr, >=, $rhs:expr) => {
        $crate::BoolExpr::Ge(Box::new($lhs), Box::new($rhs))
    };
    ($lhs:expr, ==, $rhs:expr) => {
        $crate::BoolExpr::Eq(Box::new($lhs), Box::new($rhs))
    };
}

impl Add for Expr {
    type Output = Expr;

    fn add(self, rhs: Expr) -> Self::Output {
        Expr::Add(Box::new(self), Box::new(rhs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ast() {
        // x + 1
        let expr = Expr::var("x") + Expr::lit(1);
        assert_eq!(
            expr,
            Expr::Add(Box::new(Expr::Var("x".to_string())), Box::new(Expr::Lit(1)))
        );
    }

    #[test]
    fn build_if() {
        // if x <= 10 then x + 1 else 0
        let x = Expr::var("x");
        let expr = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(10)),
            x + Expr::lit(1),
            Expr::lit(0),
        );
        assert!(matches!(expr, Expr::If(_, _, _)));
    }

    #[test]
    fn cmp_macro() {
        let x = Expr::var("x");
        let y = Expr::var("y");

        // <=
        let le = cmp!(x.clone(), <=, y.clone());
        assert!(matches!(le, BoolExpr::Le(_, _)));

        // >=
        let ge = cmp!(x.clone(), >=, y.clone());
        assert!(matches!(ge, BoolExpr::Ge(_, _)));

        // ==
        let eq = cmp!(x, ==, y);
        assert!(matches!(eq, BoolExpr::Eq(_, _)));
    }

    #[test]
    fn eval_simple() {
        let expr = Expr::var("x") + Expr::lit(1);
        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));
        assert_eq!(state.eval(&expr), 6);
        assert!(state.constraints.is_empty());
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
        let result = state.eval(&expr);

        assert_eq!(result, 6); // 5 + 1
        assert_eq!(state.constraints.len(), 1);
        // Constraint: x <= 10, took true branch
        assert_eq!(state.constraints[0].1, true);
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
        let result = state.eval(&expr);

        assert_eq!(result, 0); // else branch
        assert_eq!(state.constraints.len(), 1);
        // Constraint: x <= 10, took false branch
        assert_eq!(state.constraints[0].1, false);
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
        let result = state.eval_bool(&cond);

        assert!(result); // 3 <= 7
        assert_eq!(state.constraints.len(), 1);
        // Constraint: x <= 5, took true branch
        assert_eq!(state.constraints[0].1, true);
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
        let result = state.eval_bool(&cond);

        assert!(!result); // 10 <= 7 = false
        assert_eq!(state.constraints.len(), 1);
        // Constraint: x <= 5, took false branch
        assert_eq!(state.constraints[0].1, false);
    }
}
