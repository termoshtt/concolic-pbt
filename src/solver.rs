use std::collections::HashMap;

use crate::{BoolExpr, ConcolicState, Env, Expr};

/// Error during constraint solving
#[derive(Debug, Clone, PartialEq)]
pub enum SolverError {
    /// Bounds are unsatisfiable (e.g., x >= 10 and x <= 5)
    Unsatisfiable,
    /// Failed to find satisfying assignment after max attempts
    MaxAttemptsExceeded,
}

/// Bounds for a single variable
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bound {
    /// Lower bound (inclusive): x >= lower
    pub lower: Option<i64>,
    /// Upper bound (inclusive): x <= upper
    pub upper: Option<i64>,
    /// Values that must be excluded: x != value
    pub excluded: Vec<i64>,
}

impl Bound {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update lower bound: x >= value
    pub fn add_lower(&mut self, value: i64) {
        self.lower = Some(self.lower.map_or(value, |l| l.max(value)));
    }

    /// Update upper bound: x <= value
    pub fn add_upper(&mut self, value: i64) {
        self.upper = Some(self.upper.map_or(value, |u| u.min(value)));
    }

    /// Add excluded value: x != value
    pub fn add_excluded(&mut self, value: i64) {
        if !self.excluded.contains(&value) {
            self.excluded.push(value);
        }
    }

    /// Check if bounds are satisfiable
    pub fn is_satisfiable(&self) -> bool {
        match (self.lower, self.upper) {
            (Some(l), Some(u)) => l <= u,
            _ => true,
        }
    }

    /// Sample a random value within bounds
    pub fn sample(&self, rng: &mut impl rand::Rng) -> Option<i64> {
        let lower = self.lower.unwrap_or(-1000);
        let upper = self.upper.unwrap_or(1000);

        if lower > upper {
            return None;
        }

        // Try random sampling, avoiding excluded values
        for _ in 0..100 {
            let value = rng.random_range(lower..=upper);
            if !self.excluded.contains(&value) {
                return Some(value);
            }
        }

        // Fallback: enumerate and pick
        let valid: Vec<_> = (lower..=upper)
            .filter(|v| !self.excluded.contains(v))
            .collect();
        if valid.is_empty() {
            None
        } else {
            Some(valid[rng.random_range(0..valid.len())])
        }
    }
}

/// Collection of bounds for all variables
pub type Bounds = HashMap<String, Bound>;

/// Check if expression contains ite
fn contains_ite(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Var(_) => false,
        Expr::Add(l, r) | Expr::Sub(l, r) => contains_ite(l) || contains_ite(r),
        Expr::If(_, _, _) => true,
    }
}

/// Check if boolean expression contains ite
fn bool_contains_ite(expr: &BoolExpr) -> bool {
    match expr {
        BoolExpr::Lit(_) => false,
        BoolExpr::Le(l, r) | BoolExpr::Ge(l, r) | BoolExpr::Eq(l, r) => {
            contains_ite(l) || contains_ite(r)
        }
    }
}

/// Try to extract a single variable from expression (if it's just a variable or var + const)
fn as_single_var(expr: &Expr) -> Option<(&str, i64)> {
    match expr {
        Expr::Var(name) => Some((name, 0)),
        Expr::Add(l, r) => match (l.as_ref(), r.as_ref()) {
            (Expr::Var(name), Expr::Lit(n)) => Some((name, *n)),
            (Expr::Lit(n), Expr::Var(name)) => Some((name, *n)),
            _ => None,
        },
        Expr::Sub(l, r) => match (l.as_ref(), r.as_ref()) {
            (Expr::Var(name), Expr::Lit(n)) => Some((name, -*n)),
            _ => None,
        },
        _ => None,
    }
}

/// Extract bounds from constraints
///
/// Returns (bounds, remaining_constraints) where remaining_constraints
/// are those that couldn't be converted to simple bounds.
pub fn extract_bounds(
    constraints: &[(BoolExpr, bool)],
) -> Result<(Bounds, Vec<(BoolExpr, bool)>), SolverError> {
    let mut bounds = Bounds::new();
    let mut remaining = Vec::new();

    for (expr, taken) in constraints {
        // If contains ite, we can't extract bounds but can still evaluate
        if bool_contains_ite(expr) {
            remaining.push((expr.clone(), *taken));
            continue;
        }

        match expr {
            BoolExpr::Lit(_) => {
                // Literal constraints don't affect bounds
            }
            BoolExpr::Le(l, r) => {
                // l <= r
                // If l is "x + offset" and r is literal: x + offset <= n → x <= n - offset
                // If r is "x + offset" and l is literal: n <= x + offset → x >= n - offset
                match (as_single_var(l), as_single_var(r)) {
                    (Some((var, offset)), None) if matches!(r.as_ref(), Expr::Lit(_)) => {
                        let Expr::Lit(n) = r.as_ref() else {
                            unreachable!()
                        };
                        let bound = bounds.entry(var.to_string()).or_default();
                        if *taken {
                            // x + offset <= n → x <= n - offset
                            bound.add_upper(n - offset);
                        } else {
                            // x + offset > n → x > n - offset → x >= n - offset + 1
                            bound.add_lower(n - offset + 1);
                        }
                    }
                    (None, Some((var, offset))) if matches!(l.as_ref(), Expr::Lit(_)) => {
                        let Expr::Lit(n) = l.as_ref() else {
                            unreachable!()
                        };
                        let bound = bounds.entry(var.to_string()).or_default();
                        if *taken {
                            // n <= x + offset → x >= n - offset
                            bound.add_lower(n - offset);
                        } else {
                            // n > x + offset → x < n - offset → x <= n - offset - 1
                            bound.add_upper(n - offset - 1);
                        }
                    }
                    _ => {
                        remaining.push((expr.clone(), *taken));
                    }
                }
            }
            BoolExpr::Ge(l, r) => {
                // l >= r is equivalent to r <= l
                match (as_single_var(l), as_single_var(r)) {
                    (Some((var, offset)), None) if matches!(r.as_ref(), Expr::Lit(_)) => {
                        let Expr::Lit(n) = r.as_ref() else {
                            unreachable!()
                        };
                        let bound = bounds.entry(var.to_string()).or_default();
                        if *taken {
                            // x + offset >= n → x >= n - offset
                            bound.add_lower(n - offset);
                        } else {
                            // x + offset < n → x < n - offset → x <= n - offset - 1
                            bound.add_upper(n - offset - 1);
                        }
                    }
                    (None, Some((var, offset))) if matches!(l.as_ref(), Expr::Lit(_)) => {
                        let Expr::Lit(n) = l.as_ref() else {
                            unreachable!()
                        };
                        let bound = bounds.entry(var.to_string()).or_default();
                        if *taken {
                            // n >= x + offset → x <= n - offset
                            bound.add_upper(n - offset);
                        } else {
                            // n < x + offset → x > n - offset → x >= n - offset + 1
                            bound.add_lower(n - offset + 1);
                        }
                    }
                    _ => {
                        remaining.push((expr.clone(), *taken));
                    }
                }
            }
            BoolExpr::Eq(l, r) => {
                match (as_single_var(l), as_single_var(r)) {
                    (Some((var, offset)), None) if matches!(r.as_ref(), Expr::Lit(_)) => {
                        let Expr::Lit(n) = r.as_ref() else {
                            unreachable!()
                        };
                        let bound = bounds.entry(var.to_string()).or_default();
                        if *taken {
                            // x + offset == n → x == n - offset
                            let val = n - offset;
                            bound.add_lower(val);
                            bound.add_upper(val);
                        } else {
                            // x + offset != n → x != n - offset
                            bound.add_excluded(n - offset);
                        }
                    }
                    (None, Some((var, offset))) if matches!(l.as_ref(), Expr::Lit(_)) => {
                        let Expr::Lit(n) = l.as_ref() else {
                            unreachable!()
                        };
                        let bound = bounds.entry(var.to_string()).or_default();
                        if *taken {
                            let val = n - offset;
                            bound.add_lower(val);
                            bound.add_upper(val);
                        } else {
                            bound.add_excluded(n - offset);
                        }
                    }
                    _ => {
                        remaining.push((expr.clone(), *taken));
                    }
                }
            }
        }
    }

    // Check satisfiability
    for bound in bounds.values() {
        if !bound.is_satisfiable() {
            return Err(SolverError::Unsatisfiable);
        }
    }

    Ok((bounds, remaining))
}

/// Solver for constraint satisfaction with random sampling
pub struct Solver<R> {
    rng: R,
    max_attempts: usize,
}

impl<R: rand::Rng> Solver<R> {
    /// Create a new solver with given RNG and max attempts
    pub fn new(rng: R, max_attempts: usize) -> Self {
        Self { rng, max_attempts }
    }

    /// Try to find an input that explores an alternative path
    pub fn find_alternative(
        &mut self,
        state: &ConcolicState,
        index: usize,
    ) -> Result<Env, SolverError> {
        let negated = negate_at(&state.constraints, index);
        self.solve(&negated)
    }

    /// Solve constraints and return a satisfying assignment
    pub fn solve(&mut self, constraints: &[(BoolExpr, bool)]) -> Result<Env, SolverError> {
        let (bounds, remaining) = extract_bounds(constraints)?;

        // Collect all variable names from constraints
        let mut variables: Vec<String> = bounds.keys().cloned().collect();
        for (expr, _) in &remaining {
            collect_variables_bool(expr, &mut variables);
        }
        variables.sort();
        variables.dedup();

        self.sample(&bounds, &remaining, &variables)
    }

    /// Sample an Env that satisfies bounds and remaining constraints
    fn sample(
        &mut self,
        bounds: &Bounds,
        remaining: &[(BoolExpr, bool)],
        variables: &[String],
    ) -> Result<Env, SolverError> {
        for _ in 0..self.max_attempts {
            let mut env = Env::new();

            // Sample each variable from its bounds
            for var in variables {
                let bound = bounds.get(var).cloned().unwrap_or_default();
                match bound.sample(&mut self.rng) {
                    Some(val) => {
                        env.insert(var.clone(), val);
                    }
                    None => continue, // Try again with new sample
                }
            }

            // Check remaining constraints
            if Self::check_remaining(remaining, &env) {
                return Ok(env);
            }
        }

        Err(SolverError::MaxAttemptsExceeded)
    }

    /// Check if env satisfies all remaining constraints
    fn check_remaining(remaining: &[(BoolExpr, bool)], env: &Env) -> bool {
        for (expr, expected) in remaining {
            if expr.eval(env) != *expected {
                return false;
            }
        }
        true
    }
}

/// Collect variable names from expression
fn collect_variables(expr: &Expr, vars: &mut Vec<String>) {
    match expr {
        Expr::Lit(_) => {}
        Expr::Var(name) => {
            if !vars.contains(name) {
                vars.push(name.clone());
            }
        }
        Expr::Add(l, r) | Expr::Sub(l, r) => {
            collect_variables(l, vars);
            collect_variables(r, vars);
        }
        Expr::If(cond, then_, else_) => {
            collect_variables_bool(cond, vars);
            collect_variables(then_, vars);
            collect_variables(else_, vars);
        }
    }
}

/// Collect variable names from boolean expression
fn collect_variables_bool(expr: &BoolExpr, vars: &mut Vec<String>) {
    match expr {
        BoolExpr::Lit(_) => {}
        BoolExpr::Le(l, r) | BoolExpr::Ge(l, r) | BoolExpr::Eq(l, r) => {
            collect_variables(l, vars);
            collect_variables(r, vars);
        }
    }
}

/// Generate constraints for exploring an alternative path by negating at index i
///
/// Returns `constraints[0..i]` unchanged plus `constraints[i]` with direction negated.
/// Constraints after index i are intentionally excluded.
///
/// # Why exclude constraints after index i?
///
/// When the original path is `[T, T, F, F]` and we negate at index 2,
/// we want to explore paths starting with `[T, T, T, ...]`.
/// The 4th constraint and beyond may differ or not exist at all in the new execution.
///
/// For example, if the original code was:
/// ```text
/// if cond0 {           // T
///   if cond1 {         // T
///     if cond2 {       // F (we negate this to T)
///       // then branch: no more branches here
///     } else {
///       if cond3 { }   // F (only exists in else branch of cond2)
///     }
///   }
/// }
/// ```
///
/// When cond2 is F, we take the else branch and encounter cond3.
/// When cond2 is T (negated), we take the then branch and cond3 doesn't exist.
/// By only requiring `[T, T, T]`, we let the solver find any input satisfying
/// these constraints, and the actual execution determines what happens next.
pub fn negate_at(constraints: &[(BoolExpr, bool)], i: usize) -> Vec<(BoolExpr, bool)> {
    let mut result = constraints[0..i].to_vec();
    if i < constraints.len() {
        let (expr, taken) = &constraints[i];
        result.push((expr.clone(), !taken));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_bool_expr, parse_expr};

    #[test]
    fn bound_basic() {
        let mut bound = Bound::new();
        bound.add_lower(3);
        bound.add_upper(10);
        assert_eq!(bound.lower, Some(3));
        assert_eq!(bound.upper, Some(10));
        assert!(bound.is_satisfiable());
    }

    #[test]
    fn bound_unsatisfiable() {
        let mut bound = Bound::new();
        bound.add_lower(10);
        bound.add_upper(3);
        assert!(!bound.is_satisfiable());
    }

    #[test]
    fn extract_simple_le() {
        let constraints = vec![(parse_bool_expr("x <= 5").unwrap(), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].upper, Some(5));
        assert_eq!(bounds["x"].lower, None);
    }

    #[test]
    fn extract_le_negated() {
        let constraints = vec![(parse_bool_expr("x <= 5").unwrap(), false)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        // x > 5 → x >= 6
        assert_eq!(bounds["x"].lower, Some(6));
        assert_eq!(bounds["x"].upper, None);
    }

    #[test]
    fn extract_with_offset() {
        // x + 1 <= 5 → x <= 4
        let constraints = vec![(parse_bool_expr("x + 1 <= 5").unwrap(), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].upper, Some(4));
    }

    #[test]
    fn extract_eq() {
        let constraints = vec![(parse_bool_expr("x == 5").unwrap(), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].lower, Some(5));
        assert_eq!(bounds["x"].upper, Some(5));
    }

    #[test]
    fn extract_neq() {
        let constraints = vec![(parse_bool_expr("x == 5").unwrap(), false)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].excluded, vec![5]);
    }

    #[test]
    fn extract_two_var_goes_to_remaining() {
        let constraints = vec![(parse_bool_expr("x <= y").unwrap(), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(bounds.is_empty());
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn ite_goes_to_remaining() {
        let constraints = vec![(
            parse_bool_expr("(if x <= 5 then x else 0) <= 3").unwrap(),
            true,
        )];

        let (bounds, remaining) = extract_bounds(&constraints).unwrap();
        assert!(bounds.is_empty());
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn solver_simple() {
        // x <= 5 (true) → find x in [-1000, 5]
        let constraints = vec![(parse_bool_expr("x <= 5").unwrap(), true)];

        let mut solver = Solver::new(rand::rng(), 100);
        let env = solver.solve(&constraints).unwrap();
        assert!(env["x"] <= 5);
    }

    #[test]
    fn solver_two_var_constraint() {
        // x <= 10 (true), x <= y (true)
        // Need to find x, y such that x <= 10 and x <= y
        let constraints = vec![
            (parse_bool_expr("x <= 10").unwrap(), true),
            (parse_bool_expr("x <= y").unwrap(), true),
        ];

        let mut solver = Solver::new(rand::rng(), 1000);
        let env = solver.solve(&constraints).unwrap();
        assert!(env["x"] <= 10);
        assert!(env["x"] <= env["y"]);
    }

    #[test]
    fn negate_at_test() {
        let constraints = vec![
            (parse_bool_expr("x <= 5").unwrap(), true),
            (parse_bool_expr("x <= 10").unwrap(), true),
            (parse_bool_expr("x <= 15").unwrap(), false),
        ];

        // Negate at index 1
        let negated = negate_at(&constraints, 1);
        assert_eq!(negated.len(), 2);
        assert!(negated[0].1); // First unchanged
        assert!(!negated[1].1); // Second negated
    }

    #[test]
    fn find_alternative_test() {
        use crate::ConcolicState;
        use std::collections::HashMap;

        // Simulate: if x <= 5 then ... else ...
        // Took the then branch (x <= 5 was true)
        let expr = parse_expr("if x <= 5 then x + 1 else 0").unwrap();

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 3)]));
        state.eval(&expr);

        // Should have constraint: x <= 5 : true
        assert_eq!(state.constraints.len(), 1);
        assert!(state.constraints[0].1);

        // Find alternative (negate the constraint)
        let mut solver = Solver::new(rand::rng(), 100);
        let alt_env = solver.find_alternative(&state, 0).unwrap();

        // Should find x > 5
        assert!(alt_env["x"] > 5);
    }

    #[test]
    fn solver_with_ite() {
        // (if x <= 5 then x else 10) <= 7 : true
        // This means either (x <= 5 and x <= 7) or (x > 5 and 10 <= 7)
        // The second case is impossible (10 > 7), so x <= 5
        let constraints = vec![(
            parse_bool_expr("(if x <= 5 then x else 10) <= 7").unwrap(),
            true,
        )];

        let mut solver = Solver::new(rand::rng(), 1000);
        // Should find x such that (if x <= 5 then x else 10) <= 7
        // This requires x <= 5 (since 10 > 7)
        for _ in 0..10 {
            let env = solver.solve(&constraints).unwrap();
            assert!(env["x"] <= 5, "x = {} should be <= 5", env["x"]);
        }
    }
}
