use std::collections::HashMap;
use std::fmt;
use std::ops::{Add, Sub};

/// Environment mapping variable names to concrete values
pub type Env = HashMap<String, i64>;

/// Abstract Syntax Tree for integer expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer literal
    Lit(i64),
    /// Variable reference by name
    Var(String),
    /// Addition
    Add(Box<Expr>, Box<Expr>),
    /// Subtraction
    Sub(Box<Expr>, Box<Expr>),
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

/// Statements
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Assignment: let x = expr
    Let { name: String, expr: Expr },
    /// Assertion: assert(bool_expr)
    Assert { expr: BoolExpr },
    /// Sequence: stmt; stmt
    Seq(Box<Stmt>, Box<Stmt>),
}

impl Expr {
    pub fn lit(n: i64) -> Self {
        Expr::Lit(n)
    }

    pub fn var(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(
            name.starts_with(|c: char| c.is_ascii_alphabetic()),
            "Variable name must start with an alphabetic character: {:?}",
            name
        );
        Expr::Var(name)
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

    /// Evaluate expression with given environment
    pub fn eval(&self, env: &Env) -> i64 {
        match self {
            Expr::Lit(n) => *n,
            Expr::Var(name) => env[name],
            Expr::Add(l, r) => l.eval(env) + r.eval(env),
            Expr::Sub(l, r) => l.eval(env) - r.eval(env),
            Expr::If(cond, then_, else_) => {
                if cond.eval(env) {
                    then_.eval(env)
                } else {
                    else_.eval(env)
                }
            }
        }
    }
}

impl BoolExpr {
    pub fn lit(b: bool) -> Self {
        BoolExpr::Lit(b)
    }

    /// Evaluate boolean expression with given environment
    pub fn eval(&self, env: &Env) -> bool {
        match self {
            BoolExpr::Lit(b) => *b,
            BoolExpr::Le(l, r) => l.eval(env) <= r.eval(env),
            BoolExpr::Ge(l, r) => l.eval(env) >= r.eval(env),
            BoolExpr::Eq(l, r) => l.eval(env) == r.eval(env),
        }
    }
}

impl Add for Expr {
    type Output = Expr;

    fn add(self, rhs: Expr) -> Self::Output {
        Expr::Add(Box::new(self), Box::new(rhs))
    }
}

impl Sub for Expr {
    type Output = Expr;

    fn sub(self, rhs: Expr) -> Self::Output {
        Expr::Sub(Box::new(self), Box::new(rhs))
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Lit(n) => write!(f, "{}", n),
            Expr::Var(name) => write!(f, "{}", name),
            Expr::Add(l, r) => write!(f, "{} + {}", l, r),
            Expr::Sub(l, r) => write!(f, "{} - {}", l, r),
            Expr::If(cond, then_, else_) => {
                write!(f, "ite({}, {}, {})", cond, then_, else_)
            }
        }
    }
}

impl fmt::Display for BoolExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoolExpr::Lit(b) => write!(f, "{}", b),
            BoolExpr::Le(l, r) => write!(f, "{} <= {}", l, r),
            BoolExpr::Ge(l, r) => write!(f, "{} >= {}", l, r),
            BoolExpr::Eq(l, r) => write!(f, "{} == {}", l, r),
        }
    }
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stmt::Let { name, expr } => write!(f, "let {} = {}", name, expr),
            Stmt::Assert { expr } => write!(f, "assert({})", expr),
            Stmt::Seq(first, second) => write!(f, "{}; {}", first, second),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_bool_expr, parse_expr, parse_stmt};

    #[test]
    #[should_panic(expected = "Variable name must start with an alphabetic character")]
    fn invalid_variable_name() {
        let _ = Expr::var("123");
    }

    #[test]
    fn display_expr() {
        insta::assert_snapshot!(parse_expr("42").unwrap(), @"42");
        insta::assert_snapshot!(parse_expr("x").unwrap(), @"x");
        insta::assert_snapshot!(parse_expr("x + 1").unwrap(), @"x + 1");
        insta::assert_snapshot!(parse_expr("x - 1").unwrap(), @"x - 1");
    }

    #[test]
    fn display_bool_expr() {
        insta::assert_snapshot!(parse_bool_expr("x <= 5").unwrap(), @"x <= 5");
        insta::assert_snapshot!(parse_bool_expr("x >= 5").unwrap(), @"x >= 5");
        insta::assert_snapshot!(parse_bool_expr("x == 5").unwrap(), @"x == 5");
    }

    #[test]
    fn display_if_expr() {
        insta::assert_snapshot!(parse_expr("if x <= 5 then x else 10").unwrap(), @"ite(x <= 5, x, 10)");
        insta::assert_snapshot!(parse_bool_expr("(if x <= 5 then x else 10) <= 7").unwrap(), @"ite(x <= 5, x, 10) <= 7");
    }

    #[test]
    fn display_stmt() {
        insta::assert_snapshot!(parse_stmt("let x = 5").unwrap(), @"let x = 5");
        insta::assert_snapshot!(parse_stmt("assert(x <= 10)").unwrap(), @"assert(x <= 10)");
        insta::assert_snapshot!(parse_stmt("let y = x + 1; assert(y <= 10)").unwrap(), @"let y = x + 1; assert(y <= 10)");
    }
}
