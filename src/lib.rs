/// Abstract Syntax Tree for expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer literal
    Lit(i64),
    /// Variable reference by name
    Var(String),
    /// Addition
    Add(Box<Expr>, Box<Expr>),
}

use std::ops::Add;

impl Expr {
    pub fn lit(n: i64) -> Self {
        Expr::Lit(n)
    }

    pub fn var(name: impl Into<String>) -> Self {
        Expr::Var(name.into())
    }
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
}
