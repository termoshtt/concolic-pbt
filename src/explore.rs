use std::fmt;

use crate::{BoolExpr, ConcolicState, Env, Solver};

/// Result of exploration
#[derive(Debug, Clone, PartialEq)]
pub enum ExploreResult {
    /// Found an input that violates the property
    Counterexample(Env),
    /// Explored all reachable paths, property holds
    Verified,
    /// Reached maximum iterations without conclusive result
    MaxIterationsReached,
}

/// Path represented as sequence of branch directions
pub type Path = Vec<bool>;

/// Format path as string of T/F
fn format_path(path: &Path) -> String {
    path.iter()
        .map(|b| if *b { 'T' } else { 'F' })
        .collect()
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

    /// Find a counterexample where property evaluates to false
    ///
    /// The property is a BoolExpr that should hold for all inputs.
    /// Returns Counterexample(env) if we find an input where property is false.
    pub fn find_counterexample(&mut self, property: &BoolExpr, initial_env: Env) -> ExploreResult {
        self.explore_dfs(property, initial_env, 0)
    }

    fn explore_dfs(
        &mut self,
        property: &BoolExpr,
        env: Env,
        min_index: usize,
    ) -> ExploreResult {
        if self.iterations >= self.max_iterations {
            return ExploreResult::MaxIterationsReached;
        }
        self.iterations += 1;

        // Evaluate property with current env, collecting constraints
        // eval_bool records the property itself as a constraint
        let mut state = ConcolicState::new(env.clone());
        let property_holds = state.eval_bool(property);

        // Extract current path
        let path: Path = state.constraints.iter().map(|(_, taken)| *taken).collect();

        // Check if property is violated
        if !property_holds {
            return ExploreResult::Counterexample(env);
        }

        // Should never visit the same path twice (exploration strategy guarantees this)
        debug_assert!(
            !self.visited.iter().any(|(p, _)| p == &path),
            "BUG: visited same path twice: {:?}",
            path
        );

        // Mark this path as visited
        self.visited.push((path.clone(), env));

        // Try to explore alternative paths (depth-first: start from last constraint)
        // Only negate constraints at index >= min_index (earlier ones are handled by parent)
        for i in (min_index..state.constraints.len()).rev() {
            // Try to find an input for the alternative path
            match self.solver.find_alternative(&state, i) {
                Ok(new_env) => {
                    // Recurse with i+1 as the new min_index
                    // Child should not negate constraints at or before index i
                    // (negating at i would bring us back to the parent's path prefix)
                    let result = self.explore_dfs(property, new_env, i + 1);
                    if matches!(result, ExploreResult::Counterexample(_) | ExploreResult::MaxIterationsReached) {
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
    use crate::{cmp, Expr};
    use rand::SeedableRng;
    use std::collections::HashMap;

    #[test]
    fn find_simple_counterexample() {
        // Property: x <= 10
        // Should find counterexample where x > 10
        let x = Expr::var("x");
        let property = cmp!(x, <=, Expr::lit(10));

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&property, initial_env);

        assert!(matches!(result, ExploreResult::Counterexample(_)));
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: T
          Env: x = 5
        "###);
    }

    #[test]
    fn verify_always_true() {
        // Property: x <= x (always true)
        let x = Expr::var("x");
        let property = cmp!(x.clone(), <=, x);

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);

        let result = explorer.find_counterexample(&property, initial_env);

        assert_eq!(result, ExploreResult::Verified);
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: T
          Env: x = 5

        Unreached:
          Path: F
        "###);
    }

    #[test]
    fn explore_branching_property() {
        // Property: if x <= 5 then x + 1 <= 10 else x - 1 <= 10
        // This should hold for x in reasonable range
        let x = Expr::var("x");
        let property = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            x.clone() + Expr::lit(1),
            x.clone() - Expr::lit(1),
        )
        .le(Expr::lit(10));

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&property, initial_env);

        // Should find counterexample: x > 5 and x - 1 > 10, so x > 11
        assert!(matches!(result, ExploreResult::Counterexample(_)));
        insta::assert_snapshot!(explorer, @r###"
        Reached:
          Path: TT
          Env: x = 3

        Unreached:
          Path: TF
        "###);
    }

    #[test]
    fn unreached_path() {
        // Property: if x <= 5 then (if x >= 10 then false else true) else true
        // The path (x <= 5, true) -> (x >= 10, true) is unreachable (x <= 5 and x >= 10 is contradictory)
        let x = Expr::var("x");
        let inner = Expr::if_(
            cmp!(x.clone(), >=, Expr::lit(10)),
            Expr::lit(0), // false branch (unreachable when x <= 5)
            Expr::lit(1), // true branch
        );
        let property = Expr::if_(
            cmp!(x, <=, Expr::lit(5)),
            inner,
            Expr::lit(1),
        )
        .ge(Expr::lit(1)); // property: result >= 1

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&property, initial_env);

        assert_eq!(result, ExploreResult::Verified);
        insta::assert_snapshot!(explorer, @r#"
        Reached:
          Path: TFT
          Env: x = 3
          Path: FT
          Env: x = 149

        Unreached:
          Path: TFF
          Path: TT
          Path: FF
        "#);
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
        let x = Expr::var("x");
        let inner = Expr::if_(
            cmp!(x.clone(), >=, Expr::lit(10)),
            Expr::lit(2),
            Expr::lit(3),
        );
        let property = Expr::if_(cmp!(x, <=, Expr::lit(5)), Expr::lit(1), inner).ge(Expr::lit(1));

        let rng = rand::rngs::StdRng::seed_from_u64(42);
        let solver = Solver::new(rng, 100);
        let mut explorer = Explorer::new(solver, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);

        let result = explorer.find_counterexample(&property, initial_env);

        assert_eq!(result, ExploreResult::Verified);
        // TT has length 2, while FTT and FFT have length 3
        insta::assert_snapshot!(explorer, @r#"
        Reached:
          Path: TT
          Env: x = 3
          Path: FTT
          Env: x = 149
          Path: FFT
          Env: x = 8

        Unreached:
          Path: TF
          Path: FTF
          Path: FFF
        "#);
    }
}
