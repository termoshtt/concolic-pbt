use std::collections::HashMap;
use std::fmt;
use std::ops::{Add, Sub};

/// Environment mapping variable names to concrete values
pub type Env = HashMap<String, i64>;

/// Marker trait for expression stages
pub trait Stage: Sized {
    type Var: Clone + PartialEq + fmt::Debug + fmt::Display;
    /// Type for If expression branches (allows mixing stages in Symbolic)
    type IfBranches: Clone + PartialEq + fmt::Debug + fmt::Display;
}

/// AST stage: parsed expressions before evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ast;

impl Stage for Ast {
    type Var = String;
    type IfBranches = AstIfBranches;
}

/// Symbolic stage: expressions with SSA-converted variables
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbolic;

impl Stage for Symbolic {
    type Var = SsaVar;
    type IfBranches = SymIfBranches;
}

/// If branches for AST stage (both branches are Ast)
#[derive(Debug, Clone, PartialEq)]
pub struct AstIfBranches {
    pub then_: Box<Expr<Ast>>,
    pub else_: Box<Expr<Ast>>,
}

/// If branches for Symbolic stage (one evaluated, one not)
#[derive(Debug, Clone, PartialEq)]
pub enum SymIfBranches {
    /// Then branch was taken (evaluated to Symbolic)
    ThenTaken {
        then_: Box<Expr<Symbolic>>,
        else_: Box<Expr<Ast>>,
    },
    /// Else branch was taken (evaluated to Symbolic)
    ElseTaken {
        then_: Box<Expr<Ast>>,
        else_: Box<Expr<Symbolic>>,
    },
}

impl fmt::Display for AstIfBranches {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}", self.then_, self.else_)
    }
}

impl fmt::Display for SymIfBranches {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymIfBranches::ThenTaken { then_, else_ } => write!(f, "{}, {}", then_, else_),
            SymIfBranches::ElseTaken { then_, else_ } => write!(f, "{}, {}", then_, else_),
        }
    }
}

/// SSA version for a variable
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SsaVersion {
    /// Input variable (provided in the initial environment)
    Input,
    /// Defined by let statement (n-th definition, 1-indexed)
    Defined(std::num::NonZeroUsize),
}

impl SsaVersion {
    /// Create a new Defined version
    pub fn defined(n: usize) -> Self {
        SsaVersion::Defined(
            std::num::NonZeroUsize::new(n).expect("Defined version must be non-zero"),
        )
    }
}

impl fmt::Display for SsaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SsaVersion::Input => write!(f, "0"),
            SsaVersion::Defined(n) => write!(f, "{}", n),
        }
    }
}

/// SSA-style variable identifier: (name, version)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SsaVar {
    pub name: String,
    pub version: SsaVersion,
}

impl SsaVar {
    /// Create an input variable (version 0)
    pub fn input(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: SsaVersion::Input,
        }
    }

    /// Create a defined variable (version >= 1)
    pub fn defined(name: impl Into<String>, n: usize) -> Self {
        Self {
            name: name.into(),
            version: SsaVersion::defined(n),
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
    If(Box<BoolExpr<S>>, S::IfBranches),
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

    /// Create an if-then-else expression
    pub fn if_(cond: BoolExpr<Ast>, then_: Expr<Ast>, else_: Expr<Ast>) -> Self {
        Expr::If(
            Box::new(cond),
            AstIfBranches {
                then_: Box::new(then_),
                else_: Box::new(else_),
            },
        )
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
            Expr::If(cond, branches) => {
                if cond.eval(env) {
                    branches.then_.eval(env)
                } else {
                    branches.else_.eval(env)
                }
            }
        }
    }

    /// Collect all variable names in this expression
    pub fn collect_variables(&self, vars: &mut Vec<String>) {
        match self {
            Expr::Lit(_) => {}
            Expr::Var(name) => {
                if !vars.contains(name) {
                    vars.push(name.clone());
                }
            }
            Expr::Add(l, r) | Expr::Sub(l, r) => {
                l.collect_variables(vars);
                r.collect_variables(vars);
            }
            Expr::If(cond, branches) => {
                cond.collect_variables(vars);
                branches.then_.collect_variables(vars);
                branches.else_.collect_variables(vars);
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

    /// Collect all variable names in this expression
    pub fn collect_variables(&self, vars: &mut Vec<String>) {
        match self {
            BoolExpr::Lit(_) => {}
            BoolExpr::Le(l, r) | BoolExpr::Ge(l, r) | BoolExpr::Eq(l, r) => {
                l.collect_variables(vars);
                r.collect_variables(vars);
            }
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

impl Expr<Symbolic> {
    /// Collect all variable names in this expression
    pub fn collect_variables(&self, vars: &mut Vec<String>) {
        match self {
            Expr::Lit(_) => {}
            Expr::Var(ssa_var) => {
                if !vars.contains(&ssa_var.name) {
                    vars.push(ssa_var.name.clone());
                }
            }
            Expr::Add(l, r) | Expr::Sub(l, r) => {
                l.collect_variables(vars);
                r.collect_variables(vars);
            }
            Expr::If(cond, branches) => {
                cond.collect_variables(vars);
                match branches {
                    SymIfBranches::ThenTaken { then_, else_ } => {
                        then_.collect_variables(vars);
                        else_.collect_variables(vars);
                    }
                    SymIfBranches::ElseTaken { then_, else_ } => {
                        then_.collect_variables(vars);
                        else_.collect_variables(vars);
                    }
                }
            }
        }
    }
}

impl BoolExpr<Symbolic> {
    /// Collect all variable names in this expression
    pub fn collect_variables(&self, vars: &mut Vec<String>) {
        match self {
            BoolExpr::Lit(_) => {}
            BoolExpr::Le(l, r) | BoolExpr::Ge(l, r) | BoolExpr::Eq(l, r) => {
                l.collect_variables(vars);
                r.collect_variables(vars);
            }
        }
    }
}

impl<S: Stage> fmt::Display for Expr<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Lit(n) => write!(f, "{}", n),
            Expr::Var(name) => write!(f, "{}", name),
            Expr::Add(l, r) => write!(f, "{} + {}", l, r),
            Expr::Sub(l, r) => write!(f, "{} - {}", l, r),
            Expr::If(cond, branches) => {
                write!(f, "ite({}, {})", cond, branches)
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
