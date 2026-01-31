use std::fmt;

use crate::{find_alternative, BoolExpr, ConcolicState, Env};

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
pub struct Explorer {
    /// Paths that were actually executed with concrete inputs
    visited: Vec<(Path, Env)>,
    /// Paths where solver couldn't find a satisfying input
    unreached: Vec<Path>,
    /// Maximum number of iterations (paths to explore)
    max_iterations: usize,
    /// Maximum attempts for solver sampling
    max_solver_attempts: usize,
    /// Current iteration count
    iterations: usize,
}

impl Explorer {
    pub fn new(max_iterations: usize, max_solver_attempts: usize) -> Self {
        Self {
            visited: Vec::new(),
            unreached: Vec::new(),
            max_iterations,
            max_solver_attempts,
            iterations: 0,
        }
    }

    /// Find a counterexample where property evaluates to false
    ///
    /// The property is a BoolExpr that should hold for all inputs.
    /// Returns Counterexample(env) if we find an input where property is false.
    pub fn find_counterexample(
        &mut self,
        property: &BoolExpr,
        initial_env: Env,
        rng: &mut impl rand::Rng,
    ) -> ExploreResult {
        self.explore_dfs(property, initial_env, rng)
    }

    fn explore_dfs(
        &mut self,
        property: &BoolExpr,
        env: Env,
        rng: &mut impl rand::Rng,
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
        // This includes negating the property itself (last constraint)
        for i in (0..state.constraints.len()).rev() {
            let mut alt_path = path[..i].to_vec();
            alt_path.push(!path[i]);

            // Skip if this alternative path prefix was already visited
            // (the actual path taken may differ from alt_path due to solver limitations)
            if self.visited.iter().any(|(p, _)| p.starts_with(&alt_path)) {
                continue;
            }

            // Try to find an input for the alternative path
            match find_alternative(&state, i, rng, self.max_solver_attempts) {
                Ok(new_env) => {
                    let result = self.explore_dfs(property, new_env, rng);
                    if matches!(result, ExploreResult::Counterexample(_) | ExploreResult::MaxIterationsReached) {
                        return result;
                    }
                }
                Err(_) => {
                    // Couldn't find input (unsatisfiable or max attempts exceeded)
                    // Record as unreached for transparency
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

impl fmt::Display for Explorer {
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

        let mut explorer = Explorer::new(100, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let result = explorer.find_counterexample(&property, initial_env, &mut rng);

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

        let mut explorer = Explorer::new(100, 100);
        let initial_env = HashMap::from([("x".to_string(), 5)]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let result = explorer.find_counterexample(&property, initial_env, &mut rng);

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

        let mut explorer = Explorer::new(100, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let result = explorer.find_counterexample(&property, initial_env, &mut rng);

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

        let mut explorer = Explorer::new(100, 100);
        let initial_env = HashMap::from([("x".to_string(), 3)]);
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let result = explorer.find_counterexample(&property, initial_env, &mut rng);

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
}
