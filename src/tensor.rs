//! Tensor Shape IR for concolic execution
//!
//! This module provides types for representing tensor shape expressions
//! and constraints, separate from the main Expr/BoolExpr types.

use std::collections::HashMap;
use std::fmt;

/// Shape = non-negative integer vector
pub type Shape = Vec<usize>;

/// Environment mapping tensor variable names to their shapes
pub type ShapeEnv = HashMap<String, Shape>;

/// Tensor expression (returns a Shape when evaluated)
#[derive(Debug, Clone, PartialEq)]
pub enum TensorExpr {
    /// Tensor variable
    Var(String),
    /// Element-wise binary operation with broadcasting
    Broadcast(Box<TensorExpr>, Box<TensorExpr>),
    /// Matrix multiplication
    MatMul(Box<TensorExpr>, Box<TensorExpr>),
}

/// Shape expression (returns a usize when evaluated)
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeExpr {
    /// Integer literal
    Lit(usize),
    /// shape(tensor, dim) - the dim-th dimension of a tensor
    Shape(Box<TensorExpr>, usize),
    /// Addition
    Add(Box<ShapeExpr>, Box<ShapeExpr>),
    /// Subtraction
    Sub(Box<ShapeExpr>, Box<ShapeExpr>),
    /// Conditional expression
    If(Box<ShapeBoolExpr>, Box<ShapeExpr>, Box<ShapeExpr>),
}

/// Shape constraint (boolean expression)
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeBoolExpr {
    /// Boolean literal
    Lit(bool),
    /// Less than or equal (<=)
    Le(Box<ShapeExpr>, Box<ShapeExpr>),
    /// Greater than or equal (>=)
    Ge(Box<ShapeExpr>, Box<ShapeExpr>),
    /// Equal (==)
    Eq(Box<ShapeExpr>, Box<ShapeExpr>),
}

// ============================================================================
// Constructor helpers
// ============================================================================

impl TensorExpr {
    /// Create a tensor variable
    pub fn var(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(
            name.starts_with(|c: char| c.is_ascii_alphabetic()),
            "Variable name must start with an alphabetic character: {:?}",
            name
        );
        TensorExpr::Var(name)
    }

    /// Create a matrix multiplication expression
    pub fn matmul(a: TensorExpr, b: TensorExpr) -> Self {
        TensorExpr::MatMul(Box::new(a), Box::new(b))
    }

    /// Create a broadcast expression
    pub fn broadcast(a: TensorExpr, b: TensorExpr) -> Self {
        TensorExpr::Broadcast(Box::new(a), Box::new(b))
    }
}

impl ShapeExpr {
    /// Create an integer literal
    pub fn lit(n: usize) -> Self {
        ShapeExpr::Lit(n)
    }

    /// Create a shape access expression
    pub fn shape(tensor: TensorExpr, dim: usize) -> Self {
        ShapeExpr::Shape(Box::new(tensor), dim)
    }

    /// Create an if expression
    pub fn if_(cond: ShapeBoolExpr, then_: ShapeExpr, else_: ShapeExpr) -> Self {
        ShapeExpr::If(Box::new(cond), Box::new(then_), Box::new(else_))
    }

    /// Create a less-than-or-equal comparison
    pub fn le(self, rhs: ShapeExpr) -> ShapeBoolExpr {
        ShapeBoolExpr::Le(Box::new(self), Box::new(rhs))
    }

    /// Create a greater-than-or-equal comparison
    pub fn ge(self, rhs: ShapeExpr) -> ShapeBoolExpr {
        ShapeBoolExpr::Ge(Box::new(self), Box::new(rhs))
    }

    /// Create an equality comparison
    pub fn eq_(self, rhs: ShapeExpr) -> ShapeBoolExpr {
        ShapeBoolExpr::Eq(Box::new(self), Box::new(rhs))
    }
}

impl ShapeBoolExpr {
    /// Create a boolean literal
    pub fn lit(b: bool) -> Self {
        ShapeBoolExpr::Lit(b)
    }
}

// ============================================================================
// Arithmetic operators
// ============================================================================

impl std::ops::Add for ShapeExpr {
    type Output = ShapeExpr;

    fn add(self, rhs: ShapeExpr) -> Self::Output {
        ShapeExpr::Add(Box::new(self), Box::new(rhs))
    }
}

impl std::ops::Sub for ShapeExpr {
    type Output = ShapeExpr;

    fn sub(self, rhs: ShapeExpr) -> Self::Output {
        ShapeExpr::Sub(Box::new(self), Box::new(rhs))
    }
}

// ============================================================================
// Display implementations
// ============================================================================

impl fmt::Display for TensorExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TensorExpr::Var(name) => write!(f, "{}", name),
            TensorExpr::Broadcast(a, b) => write!(f, "broadcast({}, {})", a, b),
            TensorExpr::MatMul(a, b) => write!(f, "matmul({}, {})", a, b),
        }
    }
}

impl fmt::Display for ShapeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeExpr::Lit(n) => write!(f, "{}", n),
            ShapeExpr::Shape(tensor, dim) => write!(f, "shape({}, {})", tensor, dim),
            ShapeExpr::Add(l, r) => write!(f, "{} + {}", l, r),
            ShapeExpr::Sub(l, r) => write!(f, "{} - {}", l, r),
            ShapeExpr::If(cond, then_, else_) => {
                write!(f, "ite({}, {}, {})", cond, then_, else_)
            }
        }
    }
}

impl fmt::Display for ShapeBoolExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeBoolExpr::Lit(b) => write!(f, "{}", b),
            ShapeBoolExpr::Le(l, r) => write!(f, "{} <= {}", l, r),
            ShapeBoolExpr::Ge(l, r) => write!(f, "{} >= {}", l, r),
            ShapeBoolExpr::Eq(l, r) => write!(f, "{} == {}", l, r),
        }
    }
}

// ============================================================================
// Shape calculation helpers
// ============================================================================

/// NumPy-style broadcasting
///
/// Given two shapes, compute the broadcasted output shape.
/// Shapes are compared from the trailing dimensions.
/// Dimensions are compatible if they are equal or one of them is 1.
fn broadcast_shape(a: &Shape, b: &Shape) -> Shape {
    let max_len = a.len().max(b.len());
    let mut result = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let dim_a = if i < a.len() {
            a[a.len() - 1 - i]
        } else {
            1
        };
        let dim_b = if i < b.len() {
            b[b.len() - 1 - i]
        } else {
            1
        };

        let out_dim = if dim_a == dim_b {
            dim_a
        } else if dim_a == 1 {
            dim_b
        } else if dim_b == 1 {
            dim_a
        } else {
            panic!(
                "Cannot broadcast shapes {:?} and {:?}: dimensions {} and {} are incompatible",
                a, b, dim_a, dim_b
            );
        };

        result.push(out_dim);
    }

    result.reverse();
    result
}

/// Matrix multiplication output shape
///
/// For 2D tensors: (m, k) @ (k, n) -> (m, n)
/// For batched tensors: broadcast batch dimensions, then apply matmul to last two dims
fn matmul_shape(a: &Shape, b: &Shape) -> Shape {
    assert!(
        a.len() >= 2 && b.len() >= 2,
        "matmul requires at least 2D tensors, got shapes {:?} and {:?}",
        a,
        b
    );

    let a_k = a[a.len() - 1];
    let b_k = b[b.len() - 2];
    assert!(
        a_k == b_k,
        "matmul inner dimensions must match: got {} and {} for shapes {:?} and {:?}",
        a_k,
        b_k,
        a,
        b
    );

    let m = a[a.len() - 2];
    let n = b[b.len() - 1];

    // Handle batch dimensions
    if a.len() == 2 && b.len() == 2 {
        vec![m, n]
    } else {
        // Broadcast batch dimensions
        let a_batch = &a[..a.len() - 2];
        let b_batch = &b[..b.len() - 2];
        let batch_shape = broadcast_shape(&a_batch.to_vec(), &b_batch.to_vec());
        let mut result = batch_shape;
        result.push(m);
        result.push(n);
        result
    }
}

// ============================================================================
// Evaluation
// ============================================================================

impl TensorExpr {
    /// Evaluate the tensor expression to get its shape
    pub fn eval(&self, env: &ShapeEnv) -> Shape {
        match self {
            TensorExpr::Var(name) => env
                .get(name)
                .unwrap_or_else(|| panic!("Undefined tensor variable: {}", name))
                .clone(),
            TensorExpr::Broadcast(a, b) => {
                let shape_a = a.eval(env);
                let shape_b = b.eval(env);
                broadcast_shape(&shape_a, &shape_b)
            }
            TensorExpr::MatMul(a, b) => {
                let shape_a = a.eval(env);
                let shape_b = b.eval(env);
                matmul_shape(&shape_a, &shape_b)
            }
        }
    }
}

impl ShapeExpr {
    /// Evaluate the shape expression to get a dimension value
    pub fn eval(&self, env: &ShapeEnv) -> usize {
        match self {
            ShapeExpr::Lit(n) => *n,
            ShapeExpr::Shape(tensor, dim) => {
                let shape = tensor.eval(env);
                assert!(
                    *dim < shape.len(),
                    "Dimension {} out of bounds for shape {:?}",
                    dim,
                    shape
                );
                shape[*dim]
            }
            ShapeExpr::Add(a, b) => a.eval(env) + b.eval(env),
            ShapeExpr::Sub(a, b) => a.eval(env) - b.eval(env),
            ShapeExpr::If(cond, then_, else_) => {
                if cond.eval(env) {
                    then_.eval(env)
                } else {
                    else_.eval(env)
                }
            }
        }
    }
}

impl ShapeBoolExpr {
    /// Evaluate the boolean expression
    pub fn eval(&self, env: &ShapeEnv) -> bool {
        match self {
            ShapeBoolExpr::Lit(b) => *b,
            ShapeBoolExpr::Le(l, r) => l.eval(env) <= r.eval(env),
            ShapeBoolExpr::Ge(l, r) => l.eval(env) >= r.eval(env),
            ShapeBoolExpr::Eq(l, r) => l.eval(env) == r.eval(env),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_shape_var() {
        let env = ShapeEnv::from([("x".to_string(), vec![3, 4])]);
        let expr = ShapeExpr::shape(TensorExpr::var("x"), 0);
        assert_eq!(expr.eval(&env), 3);
    }

    #[test]
    fn eval_shape_dim1() {
        let env = ShapeEnv::from([("x".to_string(), vec![3, 4])]);
        let expr = ShapeExpr::shape(TensorExpr::var("x"), 1);
        assert_eq!(expr.eval(&env), 4);
    }

    #[test]
    fn eval_matmul_shape() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![2, 3]),
            ("b".to_string(), vec![3, 4]),
        ]);
        let matmul = TensorExpr::matmul(TensorExpr::var("a"), TensorExpr::var("b"));
        assert_eq!(matmul.eval(&env), vec![2, 4]);
    }

    #[test]
    fn eval_broadcast_shape() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![3, 1]),
            ("b".to_string(), vec![1, 4]),
        ]);
        let bc = TensorExpr::broadcast(TensorExpr::var("a"), TensorExpr::var("b"));
        assert_eq!(bc.eval(&env), vec![3, 4]);
    }

    #[test]
    fn eval_broadcast_different_dims() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![3, 4]),
            ("b".to_string(), vec![4]),
        ]);
        let bc = TensorExpr::broadcast(TensorExpr::var("a"), TensorExpr::var("b"));
        assert_eq!(bc.eval(&env), vec![3, 4]);
    }

    #[test]
    fn eval_broadcast_scalar() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![3, 4]),
            ("b".to_string(), vec![1]),
        ]);
        let bc = TensorExpr::broadcast(TensorExpr::var("a"), TensorExpr::var("b"));
        assert_eq!(bc.eval(&env), vec![3, 4]);
    }

    #[test]
    fn eval_matmul_batched() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![2, 3, 4]),
            ("b".to_string(), vec![2, 4, 5]),
        ]);
        let matmul = TensorExpr::matmul(TensorExpr::var("a"), TensorExpr::var("b"));
        assert_eq!(matmul.eval(&env), vec![2, 3, 5]);
    }

    #[test]
    fn eval_matmul_broadcast_batch() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![1, 3, 4]),
            ("b".to_string(), vec![2, 4, 5]),
        ]);
        let matmul = TensorExpr::matmul(TensorExpr::var("a"), TensorExpr::var("b"));
        assert_eq!(matmul.eval(&env), vec![2, 3, 5]);
    }

    #[test]
    fn eval_shape_expr_arithmetic() {
        let env = ShapeEnv::from([("x".to_string(), vec![3, 4])]);
        let dim0 = ShapeExpr::shape(TensorExpr::var("x"), 0);
        let dim1 = ShapeExpr::shape(TensorExpr::var("x"), 1);
        let sum = dim0 + dim1;
        assert_eq!(sum.eval(&env), 7);
    }

    #[test]
    fn eval_shape_bool_expr() {
        let env = ShapeEnv::from([("x".to_string(), vec![3, 4])]);
        let dim0 = ShapeExpr::shape(TensorExpr::var("x"), 0);
        let cond = dim0.ge(ShapeExpr::lit(3));
        assert!(cond.eval(&env));
    }

    #[test]
    fn eval_shape_if() {
        let env = ShapeEnv::from([("x".to_string(), vec![3, 4])]);
        let dim0 = ShapeExpr::shape(TensorExpr::var("x"), 0);
        let cond = dim0.ge(ShapeExpr::lit(5));
        let expr = ShapeExpr::if_(cond, ShapeExpr::lit(10), ShapeExpr::lit(20));
        assert_eq!(expr.eval(&env), 20);
    }

    #[test]
    fn display_tensor_expr() {
        let x = TensorExpr::var("x");
        let y = TensorExpr::var("y");
        assert_eq!(format!("{}", x), "x");
        assert_eq!(
            format!("{}", TensorExpr::matmul(x.clone(), y.clone())),
            "matmul(x, y)"
        );
        assert_eq!(
            format!("{}", TensorExpr::broadcast(x, y)),
            "broadcast(x, y)"
        );
    }

    #[test]
    fn display_shape_expr() {
        let x = TensorExpr::var("x");
        let shape0 = ShapeExpr::shape(x, 0);
        assert_eq!(format!("{}", shape0), "shape(x, 0)");
        assert_eq!(format!("{}", ShapeExpr::lit(42)), "42");
    }

    #[test]
    fn display_shape_bool_expr() {
        let x = TensorExpr::var("x");
        let shape0 = ShapeExpr::shape(x, 0);
        let cond = shape0.ge(ShapeExpr::lit(3));
        assert_eq!(format!("{}", cond), "shape(x, 0) >= 3");
    }

    #[test]
    #[should_panic(expected = "Variable name must start with an alphabetic character")]
    fn invalid_variable_name() {
        let _ = TensorExpr::var("123");
    }

    #[test]
    #[should_panic(expected = "Cannot broadcast shapes")]
    fn broadcast_incompatible() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![3, 4]),
            ("b".to_string(), vec![5, 6]),
        ]);
        let bc = TensorExpr::broadcast(TensorExpr::var("a"), TensorExpr::var("b"));
        bc.eval(&env);
    }

    #[test]
    #[should_panic(expected = "inner dimensions must match")]
    fn matmul_dimension_mismatch() {
        let env = ShapeEnv::from([
            ("a".to_string(), vec![2, 3]),
            ("b".to_string(), vec![4, 5]),
        ]);
        let matmul = TensorExpr::matmul(TensorExpr::var("a"), TensorExpr::var("b"));
        matmul.eval(&env);
    }

    #[test]
    #[should_panic(expected = "Dimension 2 out of bounds")]
    fn shape_dim_out_of_bounds() {
        let env = ShapeEnv::from([("x".to_string(), vec![3, 4])]);
        let expr = ShapeExpr::shape(TensorExpr::var("x"), 2);
        expr.eval(&env);
    }
}
