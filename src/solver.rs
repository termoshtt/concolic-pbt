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

/// Solver for constraint satisfaction
pub struct Solver {
    bounds: Bounds,
    remaining: Vec<(BoolExpr, bool)>,
    variables: Vec<String>,
}

impl Solver {
    /// Create solver from constraints
    pub fn new(constraints: &[(BoolExpr, bool)]) -> Result<Self, SolverError> {
        let (bounds, remaining) = extract_bounds(constraints)?;

        // Collect all variable names from constraints
        let mut variables: Vec<String> = bounds.keys().cloned().collect();
        for (expr, _) in &remaining {
            collect_variables_bool(expr, &mut variables);
        }
        variables.sort();
        variables.dedup();

        Ok(Self {
            bounds,
            remaining,
            variables,
        })
    }

    /// Sample an Env that satisfies all constraints
    pub fn sample(
        &self,
        rng: &mut impl rand::Rng,
        max_attempts: usize,
    ) -> Result<Env, SolverError> {
        for _ in 0..max_attempts {
            let mut env = Env::new();

            // Sample each variable from its bounds
            for var in &self.variables {
                let bound = self.bounds.get(var).cloned().unwrap_or_default();
                match bound.sample(rng) {
                    Some(val) => {
                        env.insert(var.clone(), val);
                    }
                    None => continue, // Try again with new sample
                }
            }

            // Check remaining constraints
            if self.check_remaining(&env) {
                return Ok(env);
            }
        }

        Err(SolverError::MaxAttemptsExceeded)
    }

    /// Check if env satisfies all remaining constraints
    fn check_remaining(&self, env: &Env) -> bool {
        for (expr, expected) in &self.remaining {
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

/// Generate alternative path by negating the constraint at index i
///
/// Returns constraints[0..i] with constraints[i] negated
pub fn negate_at(constraints: &[(BoolExpr, bool)], i: usize) -> Vec<(BoolExpr, bool)> {
    let mut result = constraints[0..i].to_vec();
    if i < constraints.len() {
        let (expr, taken) = &constraints[i];
        result.push((expr.clone(), !taken));
    }
    result
}

/// Try to find an input that explores an alternative path
pub fn find_alternative(
    state: &ConcolicState,
    index: usize,
    rng: &mut impl rand::Rng,
    max_attempts: usize,
) -> Result<Env, SolverError> {
    let negated = negate_at(&state.constraints, index);
    let solver = Solver::new(&negated)?;
    solver.sample(rng, max_attempts)
}

/// Try all alternative paths and return the first successful one
pub fn find_any_alternative(
    state: &ConcolicState,
    rng: &mut impl rand::Rng,
    max_attempts: usize,
) -> Option<(usize, Env)> {
    for i in (0..state.constraints.len()).rev() {
        match find_alternative(state, i, rng, max_attempts) {
            Ok(env) => return Some((i, env)),
            Err(_) => continue,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmp;

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
        let x = Expr::var("x");
        let constraints = vec![(cmp!(x, <=, Expr::lit(5)), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].upper, Some(5));
        assert_eq!(bounds["x"].lower, None);
    }

    #[test]
    fn extract_le_negated() {
        let x = Expr::var("x");
        let constraints = vec![(cmp!(x, <=, Expr::lit(5)), false)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        // x > 5 → x >= 6
        assert_eq!(bounds["x"].lower, Some(6));
        assert_eq!(bounds["x"].upper, None);
    }

    #[test]
    fn extract_with_offset() {
        let x = Expr::var("x");
        // x + 1 <= 5 → x <= 4
        let constraints = vec![(cmp!(x + Expr::lit(1), <=, Expr::lit(5)), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].upper, Some(4));
    }

    #[test]
    fn extract_eq() {
        let x = Expr::var("x");
        let constraints = vec![(cmp!(x, ==, Expr::lit(5)), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].lower, Some(5));
        assert_eq!(bounds["x"].upper, Some(5));
    }

    #[test]
    fn extract_neq() {
        let x = Expr::var("x");
        let constraints = vec![(cmp!(x, ==, Expr::lit(5)), false)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(bounds["x"].excluded, vec![5]);
    }

    #[test]
    fn extract_two_var_goes_to_remaining() {
        let x = Expr::var("x");
        let y = Expr::var("y");
        let constraints = vec![(cmp!(x, <=, y), true)];
        let (bounds, remaining) = extract_bounds(&constraints).unwrap();

        assert!(bounds.is_empty());
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn ite_goes_to_remaining() {
        let x = Expr::var("x");
        let expr = Expr::if_(cmp!(x.clone(), <=, Expr::lit(5)), x, Expr::lit(0));
        let constraints = vec![(cmp!(expr, <=, Expr::lit(3)), true)];

        let (bounds, remaining) = extract_bounds(&constraints).unwrap();
        assert!(bounds.is_empty());
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn solver_simple() {
        // x <= 5 (true) → find x in [-1000, 5]
        let x = Expr::var("x");
        let constraints = vec![(cmp!(x, <=, Expr::lit(5)), true)];
        let solver = Solver::new(&constraints).unwrap();

        let mut rng = rand::rng();
        let env = solver.sample(&mut rng, 100).unwrap();
        assert!(env["x"] <= 5);
    }

    #[test]
    fn solver_two_var_constraint() {
        // x <= 10 (true), x <= y (true)
        // Need to find x, y such that x <= 10 and x <= y
        let x = Expr::var("x");
        let y = Expr::var("y");
        let constraints = vec![
            (cmp!(x.clone(), <=, Expr::lit(10)), true),
            (cmp!(x, <=, y), true),
        ];
        let solver = Solver::new(&constraints).unwrap();

        let mut rng = rand::rng();
        let env = solver.sample(&mut rng, 1000).unwrap();
        assert!(env["x"] <= 10);
        assert!(env["x"] <= env["y"]);
    }

    #[test]
    fn negate_at_test() {
        let x = Expr::var("x");
        let constraints = vec![
            (cmp!(x.clone(), <=, Expr::lit(5)), true),
            (cmp!(x.clone(), <=, Expr::lit(10)), true),
            (cmp!(x, <=, Expr::lit(15)), false),
        ];

        // Negate at index 1
        let negated = negate_at(&constraints, 1);
        assert_eq!(negated.len(), 2);
        assert_eq!(negated[0].1, true); // First unchanged
        assert_eq!(negated[1].1, false); // Second negated
    }

    #[test]
    fn find_alternative_test() {
        use crate::ConcolicState;
        use std::collections::HashMap;

        // Simulate: if x <= 5 then ... else ...
        // Took the then branch (x <= 5 was true)
        let x = Expr::var("x");
        let expr = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            x + Expr::lit(1),
            Expr::lit(0),
        );

        let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 3)]));
        state.eval(&expr);

        // Should have constraint: x <= 5 : true
        assert_eq!(state.constraints.len(), 1);
        assert_eq!(state.constraints[0].1, true);

        // Find alternative (negate the constraint)
        let mut rng = rand::rng();
        let alt_env = find_alternative(&state, 0, &mut rng, 100).unwrap();

        // Should find x > 5
        assert!(alt_env["x"] > 5);
    }

    #[test]
    fn solver_with_ite() {
        // (if x <= 5 then x else 10) <= 7 : true
        // This means either (x <= 5 and x <= 7) or (x > 5 and 10 <= 7)
        // The second case is impossible (10 > 7), so x <= 5
        let x = Expr::var("x");
        let inner = Expr::if_(
            cmp!(x.clone(), <=, Expr::lit(5)),
            x,
            Expr::lit(10),
        );
        let constraints = vec![(cmp!(inner, <=, Expr::lit(7)), true)];
        let solver = Solver::new(&constraints).unwrap();

        let mut rng = rand::rng();
        // Should find x such that (if x <= 5 then x else 10) <= 7
        // This requires x <= 5 (since 10 > 7)
        for _ in 0..10 {
            let env = solver.sample(&mut rng, 1000).unwrap();
            assert!(env["x"] <= 5, "x = {} should be <= 5", env["x"]);
        }
    }
}
