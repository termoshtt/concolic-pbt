use std::fmt;

use crate::{BoolExpr, ConcolicState, Env, OracleFailure, Solver, Stmt, Stmts};

/// Result of exploration
#[derive(Debug, Clone, PartialEq)]
pub enum ExploreResult {
    /// Found an input that violates the property
    Counterexample {
        /// The input that caused the failure
        env: Env,
        /// The oracle failure (e.g., assertion failure)
        failure: OracleFailure,
    },
    /// Explored all reachable paths, property holds
    Verified,
    /// Reached maximum iterations without conclusive result
    MaxIterationsReached,
}

/// Path represented as sequence of branch directions
pub type Path = Vec<bool>;

/// Format path as string of T/F
fn format_path(path: &Path) -> String {
    path.iter().map(|b| if *b { 'T' } else { 'F' }).collect()
}

/// Format env as "x = 1, y = 2"
fn format_env(env: &Env) -> String {
    let mut vars: Vec<_> = env.iter().collect();
    vars.sort_by_key(|(k, _)| *k);
    vars.iter()
        .map(|(k, v)| format!("{} = {}", k, v))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Depth-first explorer for finding property violations
pub struct Explorer<R> {
    /// Solver for finding alternative paths
    solver: Solver<R>,
    /// Paths that were actually executed with concrete inputs
    visited: Vec<(Path, Env)>,
    /// Paths where solver couldn't find a satisfying input
    unreached: Vec<Path>,
    /// Maximum number of iterations (paths to explore)
    max_iterations: usize,
    /// Current iteration count
    iterations: usize,
}

impl<R: rand::Rng> Explorer<R> {
    pub fn new(solver: Solver<R>, max_iterations: usize) -> Self {
        Self {
            solver,
            visited: Vec::new(),
            unreached: Vec::new(),
            max_iterations,
            iterations: 0,
        }
    }

    /// Find a counterexample where any assertion fails
    ///
    /// Returns Counterexample(env) if we find an input where an assertion is violated.
    pub fn find_counterexample(&mut self, stmts: &Stmts, initial_env: Env) -> ExploreResult {
        self.explore_dfs(stmts, initial_env, 0)
    }

    fn explore_dfs(&mut self, stmts: &Stmts, env: Env, min_index: usize) -> ExploreResult {
        if self.iterations >= self.max_iterations {
            return ExploreResult::MaxIterationsReached;
        }
        self.iterations += 1;

        // Execute statements with current env, collecting path constraints
        let mut state = ConcolicState::new(env.clone());
        let result = state.exec_stmts(stmts);

        // Extract current path
        let path: Path = state
            .path_constraints
            .iter()
            .map(|(_, taken)| *taken)
            .collect();

        // Check if assertion failed
        if let Err(failure) = result {
            return ExploreResult::Counterexample { env, failure };
        }

        // Should never visit the same path twice (exploration strategy guarantees this)
        debug_assert!(
            !self.visited.iter().any(|(p, _)| p == &path),
            "BUG: visited same path twice: {:?}",
            path
        );

        // Mark this path as visited
        self.visited.push((path.clone(), env));

        // Exploration strategy:
        // 1. First, try to negate each assertion on the current path
        //    Solve: path_constraints AND let_constraints AND NOT(assertion)
        // 2. If that fails, try alternative paths by negating branch conditions

        // Collect all assertions from the statements
        let assertions: Vec<&BoolExpr> = stmts
            .0
            .iter()
            .filter_map(|s| match s {
                Stmt::Assert { expr } => Some(expr),
                Stmt::Let { .. } => None,
            })
            .collect();

        // Step 1: Try to find counterexample on this path by negating each assertion
        for assertion in &assertions {
            let mut constraints = state.path_constraints.clone();
            // Add let constraints as equality constraints
            for (ssa_var, expr) in &state.let_constraints {
                constraints.push((
                    BoolExpr::Eq(
                        Box::new(crate::Expr::Var(ssa_var.to_string())),
                        Box::new(expr.clone()),
                    ),
                    true,
                ));
            }
            // Convert assertion to SSA form
            let ssa_assertion = state.to_ssa_bool_expr(assertion);
            constraints.push((ssa_assertion, false));

            if let Ok(new_env) = self.solver.solve(&constraints) {
                // Verify the counterexample (solver might give approximate solution)
                let mut new_state = ConcolicState::new(new_env.clone());
                if new_state.exec_stmts(stmts).is_err() {
                    return ExploreResult::Counterexample {
                        env: new_env,
                        failure: OracleFailure::AssertionFailed {
                            expr: (*assertion).clone(),
                        },
                    };
                }
                // Solver gave input that doesn't actually violate assertion; continue
            }
        }

        // Step 2: Try alternative paths (depth-first: start from last constraint)
        // Only negate constraints at index >= min_index (earlier ones are handled by parent)
        for i in (min_index..state.path_constraints.len()).rev() {
            // Try to find an input for the alternative path
            match self.solver.find_alternative(&state, i) {
                Ok(new_env) => {
                    // Recurse with i+1 as the new min_index
                    // Child should not negate constraints at or before index i
                    // (negating at i would bring us back to the parent's path prefix)
                    let result = self.explore_dfs(stmts, new_env, i + 1);
                    if matches!(
                        result,
                        ExploreResult::Counterexample { .. } | ExploreResult::MaxIterationsReached
                    ) {
                        return result;
                    }
                }
                Err(_) => {
                    // Couldn't find input (unsatisfiable or max attempts exceeded)
                    let mut alt_path = path[..i].to_vec();
                    alt_path.push(!path[i]);
                    self.unreached.push(alt_path);
                }
            }
        }

        ExploreResult::Verified
    }

    /// Get the number of paths explored
    pub fn iterations(&self) -> usize {
        self.iterations
    }

    /// Get the number of unique paths visited
    pub fn paths_visited(&self) -> usize {
        self.visited.len()
    }

    /// Get the number of paths where solver couldn't find input
    pub fn paths_unreached(&self) -> usize {
        self.unreached.len()
    }
}

impl<R> fmt::Display for Explorer<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Reached:")?;
        for (path, env) in &self.visited {
            writeln!(f, "  Path: {}", format_path(path))?;
            writeln!(f, "  Env: {}", format_env(env))?;
        }

        if !self.unreached.is_empty() {
            writeln!(f)?;
            writeln!(f, "Unreached:")?;
            for path in &self.unreached {
                writeln!(f, "  Path: {}", format_path(path))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_bool_expr;
    use rand::SeedableRng;
    use std::collections::HashMap;

    /// Helper to create Stmts from a single assert
    fn assert_stmts(expr: &str) -> Stmts {
        Stmt::assert(parse_bool_expr(expr).unwrap()).into()
    }

    #[test]
    fn find_simple_counterexample() {
        // Property: x <= 10
        // Should find counterexample where x > 10
        let stmts = assert_stmts("x <= 10");

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        assert!(matches!(result, ExploreResult::Counterexample { .. }));
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: 
          Env: x = 5
        "###);
    }

    #[test]
    fn verify_always_true() {
        // Property: x <= x (always true)
        let stmts = assert_stmts("x <= x");

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        assert_eq!(result, ExploreResult::Verified);
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: 
          Env: x = 5
        "###);
    }

    #[test]
    fn explore_branching_property() {
        // Property: (if x <= 5 then x + 1 else x - 1) <= 10
        // This should hold for x in reasonable range
        let stmts = assert_stmts("(if x <= 5 then x + 1 else x - 1) <= 10");

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        // Should find counterexample: x > 5 and x - 1 > 10, so x > 11
        assert!(matches!(result, ExploreResult::Counterexample { .. }));
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: T
          Env: x = 3
        "###);
    }

    #[test]
    fn unreached_path() {
        // Property: (if x <= 5 then (if x >= 10 then 0 else 1) else 1) >= 1
        // The path (x <= 5, true) -> (x >= 10, true) is unreachable (x <= 5 and x >= 10 is contradictory)
        let stmts = assert_stmts("(if x <= 5 then (if x >= 10 then 0 else 1) else 1) >= 1");

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        assert_eq!(result, ExploreResult::Verified);
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: TF
          Env: x = 3
          Path: F
          Env: x = 149

        Unreached:
          Path: TT
        "###);
    }

    #[test]
    fn path_length_changes_on_alternative() {
        // Property: (if x <= 5 then 1 else (if x >= 10 then 2 else 3)) >= 1
        //
        // When x <= 5 (then branch): result = 1, no inner branch
        //   Path: [T, T] (x<=5, result>=1)
        //
        // When x > 5 (else branch): inner branch on x >= 10 exists
        //   Path: [F, T, T] or [F, F, T] (x<=5, x>=10, result>=1)
        //
        // This demonstrates that negating at index 0 can lead to a longer path.
        let stmts = assert_stmts("(if x <= 5 then 1 else (if x >= 10 then 2 else 3)) >= 1");

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        assert_eq!(result, ExploreResult::Verified);
        // TT has length 2, while FTT and FFT have length 3
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: T
          Env: x = 3
          Path: FT
          Env: x = 149
          Path: FF
          Env: x = 8
        "###);
    }

    #[test]
    fn counterexample_includes_assertion_failure() {
        // Verify that counterexample includes OracleFailure::AssertionFailed
        let stmts = assert_stmts("x <= 10");

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        match result {
            ExploreResult::Counterexample { env, failure } => {
                assert!(env["x"] > 10);
                assert!(matches!(
                    failure,
                    crate::OracleFailure::AssertionFailed { .. }
                ));
            }
            _ => panic!("Expected Counterexample"),
        }
    }

    #[test]
    fn explore_with_let() {
        // let y = x + 1; assert(y <= 10)
        // y = x + 1 means x + 1 <= 10, so x <= 9
        // Should find counterexample where x > 9
        let stmts = crate::parse_stmts("let y = x + 1; assert(y <= 10)").unwrap();

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        match result {
            ExploreResult::Counterexample { env, .. } => {
                // x should be > 9 (so y = x + 1 > 10)
                assert!(env["x"] > 9, "x = {} should be > 9", env["x"]);
            }
            _ => panic!("Expected Counterexample, got {:?}", result),
        }
    }

    #[test]
    fn explore_with_let_and_if() {
        // let y = if x >= 1 then x else x + 1; assert(y <= 5)
        // When x >= 1: y = x, need x > 5
        // When x < 1: y = x + 1, need x + 1 > 5, so x > 4 (but x < 1, impossible)
        // So counterexample requires x >= 1 and x > 5
        let stmts =
            crate::parse_stmts("let y = if x >= 1 then x else x + 1; assert(y <= 5)").unwrap();

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        match result {
            ExploreResult::Counterexample { env, .. } => {
                // x should be > 5 and >= 1 (so y = x > 5)
                assert!(env["x"] > 5, "x = {} should be > 5", env["x"]);
            }
            _ => panic!("Expected Counterexample, got {:?}", result),
        }
    }

    #[test]
    fn explore_with_shadowing_let() {
        // let y = x + 1; let y = y + 1; assert(y <= 10)
        // y = x + 2, so need x + 2 > 10, i.e., x > 8
        let stmts = crate::parse_stmts("let y = x + 1; let y = y + 1; assert(y <= 10)").unwrap();

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&stmts, initial_env);

        match result {
            ExploreResult::Counterexample { env, .. } => {
                // x should be > 8 (so y = x + 2 > 10)
                assert!(env["x"] > 8, "x = {} should be > 8", env["x"]);
            }
            _ => panic!("Expected Counterexample, got {:?}", result),
        }
    }
}
