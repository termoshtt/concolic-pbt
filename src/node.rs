use std::ops::Add;

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

impl Add for Expr {
    type Output = Expr;

    fn add(self, rhs: Expr) -> Self::Output {
        Expr::Add(Box::new(self), Box::new(rhs))
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
