use std::collections::HashMap;
use std::fmt;
use std::ops::{Add, Sub};

/// Environment mapping variable names to concrete values
pub type Env = HashMap<String, i64>;

/// Marker trait for expression stages
pub trait Stage {
    type Var: Clone + PartialEq + fmt::Debug + fmt::Display;
}

/// AST stage: parsed expressions before evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ast;

impl Stage for Ast {
    type Var = String;
}

/// Symbolic stage: expressions with SSA-converted variables
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbolic;

impl Stage for Symbolic {
    type Var = SsaVar;
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

impl fmt::Display for SsaVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// Abstract Syntax Tree for integer expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr<S: Stage = Ast> {
    /// Integer literal
    Lit(i64),
    /// Variable reference
    Var(S::Var),
    /// Addition
    Add(Box<Expr<S>>, Box<Expr<S>>),
    /// Subtraction
    Sub(Box<Expr<S>>, Box<Expr<S>>),
    /// Conditional expression
    If(Box<BoolExpr<S>>, Box<Expr<S>>, Box<Expr<S>>),
}

/// Boolean expressions
#[derive(Debug, Clone, PartialEq)]
pub enum BoolExpr<S: Stage = Ast> {
    /// Boolean literal
    Lit(bool),
    /// Less than or equal (<=)
    Le(Box<Expr<S>>, Box<Expr<S>>),
    /// Greater than or equal (>=)
    Ge(Box<Expr<S>>, Box<Expr<S>>),
    /// Equal (==)
    Eq(Box<Expr<S>>, Box<Expr<S>>),
}

/// Single statement (always in AST stage)
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Assertion: assert(bool_expr)
    Assert { expr: BoolExpr<Ast> },
    /// Variable binding: let name = expr
    ///
    /// The binding introduces a constraint `name == expr` for the solver,
    /// without expanding `name` in subsequent expressions.
    Let { name: String, expr: Expr<Ast> },
}

/// Sequence of statements
#[derive(Debug, Clone, PartialEq)]
pub struct Stmts(pub Vec<Stmt>);

impl From<Stmt> for Stmts {
    fn from(stmt: Stmt) -> Self {
        Stmts(vec![stmt])
    }
}

impl<T: Into<Stmt>> FromIterator<T> for Stmts {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Stmts(iter.into_iter().map(Into::into).collect())
    }
}

impl Stmt {
    /// Create an assertion statement
    pub fn assert(expr: BoolExpr<Ast>) -> Self {
        Stmt::Assert { expr }
    }

    /// Create a let binding statement
    pub fn let_(name: impl Into<String>, expr: Expr<Ast>) -> Self {
        Stmt::Let {
            name: name.into(),
            expr,
        }
    }
}

impl Expr<Ast> {
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

    pub fn if_(cond: BoolExpr<Ast>, then_: Expr<Ast>, else_: Expr<Ast>) -> Self {
        Expr::If(Box::new(cond), Box::new(then_), Box::new(else_))
    }

    pub fn le(self, rhs: Expr<Ast>) -> BoolExpr<Ast> {
        BoolExpr::Le(Box::new(self), Box::new(rhs))
    }

    pub fn ge(self, rhs: Expr<Ast>) -> BoolExpr<Ast> {
        BoolExpr::Ge(Box::new(self), Box::new(rhs))
    }

    pub fn eq_(self, rhs: Expr<Ast>) -> BoolExpr<Ast> {
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

impl BoolExpr<Ast> {
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

impl Add for Expr<Ast> {
    type Output = Expr<Ast>;

    fn add(self, rhs: Expr<Ast>) -> Self::Output {
        Expr::Add(Box::new(self), Box::new(rhs))
    }
}

impl Sub for Expr<Ast> {
    type Output = Expr<Ast>;

    fn sub(self, rhs: Expr<Ast>) -> Self::Output {
        Expr::Sub(Box::new(self), Box::new(rhs))
    }
}

impl<S: Stage> fmt::Display for Expr<S> {
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

impl<S: Stage> fmt::Display for BoolExpr<S> {
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
            Stmt::Assert { expr } => write!(f, "assert({})", expr),
            Stmt::Let { name, expr } => write!(f, "let {} = {}", name, expr),
        }
    }
}

impl fmt::Display for Stmts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, stmt) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{}", stmt)?;
        }
        Ok(())
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
    use crate::{parse_bool_expr, parse_expr, parse_stmts};

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
    fn display_stmts() {
        insta::assert_snapshot!(parse_stmts("assert(x <= 10)").unwrap(), @"assert(x <= 10)");
        insta::assert_snapshot!(parse_stmts("assert(x >= 0); assert(x <= 10)").unwrap(), @"assert(x >= 0); assert(x <= 10)");
    }

    #[test]
    fn display_let_stmt() {
        insta::assert_snapshot!(parse_stmts("let y = x + 1").unwrap(), @"let y = x + 1");
        insta::assert_snapshot!(
            parse_stmts("let y = if x >= 1 then x else x + 1").unwrap(),
            @"let y = ite(x >= 1, x, x + 1)"
        );
        insta::assert_snapshot!(
            parse_stmts("let y = x + 1; assert(y <= 10)").unwrap(),
            @"let y = x + 1; assert(y <= 10)"
        );
    }
}
