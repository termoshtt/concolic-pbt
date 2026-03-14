use concolic_pbt::{ConcolicState, ExploreResult, Explorer, Solver, parse_bool_expr};
use rand::SeedableRng;
use std::collections::HashMap;

#[test]
fn find_assertion_failure_simple() {
    // Property: x <= 10
    // We want to find x > 10
    let property = parse_bool_expr("x <= 10").unwrap();
    let rng = rand::rngs::StdRng::seed_from_u64(42);
    let mut solver = Solver::new(rng, 100);

    let result = solver.find_assertion_failure(&[], &property);
    println!("result: {:?}", result);

    assert!(result.is_ok(), "Should find x > 10");
    let env = result.unwrap();
    assert!(env["x"] > 10, "x should be > 10, got {}", env["x"]);
}

#[test]
fn explore_finds_counterexample() {
    let property = parse_bool_expr("x <= 10").unwrap();
    let rng = rand::rngs::StdRng::seed_from_u64(42);
    let solver = Solver::new(rng, 100);
    let mut explorer = Explorer::new(solver, 100);
    let initial_env = HashMap::from([("x".to_string(), 5)]);

    let result = explorer.find_counterexample(&property, initial_env);
    println!("explore result: {:?}", result);

    match &result {
        ExploreResult::Counterexample { env, failures } => {
            println!("Found counterexample: {:?}", env);
            println!("Failures: {:?}", failures);
            assert!(env["x"] > 10);
        }
        ExploreResult::Verified => {
            panic!("Should have found counterexample, got Verified");
        }
        ExploreResult::MaxIterationsReached => {
            panic!("Should have found counterexample, got MaxIterationsReached");
        }
    }
}

#[test]
fn eval_bool_pure_does_not_record() {
    let property = parse_bool_expr("x <= 10").unwrap();
    let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 5)]));

    let result = state.eval_bool_pure(&property);
    println!("eval_bool_pure result: {}", result);
    println!("path_constraints: {:?}", state.path_constraints);

    assert!(result);
    assert!(
        state.path_constraints.is_empty(),
        "eval_bool_pure should not record path constraints"
    );
}
