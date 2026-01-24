use std::collections::HashMap;

use concolic_pbt::{cmp, ConcolicState, Expr};

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
