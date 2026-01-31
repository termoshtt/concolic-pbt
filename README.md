# concolic-pbt

A learning project exploring concolic execution for property-based testing.

## Motivation

Property-Based Testing (PBT) and fuzzing are fundamentally about solving satisfiability problems. Their basic strategy is brute-force search over the input space, but we can do better by incorporating feedback from program execution.

One such feedback mechanism is **concolic execution** (concrete + symbolic). The approach works as follows:

1. Execute the program with a random initial input
2. During execution, record each branch condition as both:
   - The concrete value (true/false) taken
   - A symbolic formula over the input variables
3. Use an SMT solver to find new inputs that satisfy alternative branch conditions, exploring different execution paths

This is a white-box testing technique that requires representing the program under test in an analyzable form—typically an intermediate language.

## Current Implementation

### Expression Language

A minimal expression language with:

- **Integer expressions**: literals, variables, addition, subtraction, conditional (`if-then-else`)
- **Boolean expressions**: comparisons (`<=`, `>=`, `==`)

```rust
use concolic_pbt::{Expr, cmp};

let x = Expr::var("x");
let expr = Expr::if_(
    cmp!(x.clone(), <=, Expr::lit(5)),
    x.clone() + Expr::lit(1),
    x - Expr::lit(1),
);
```

### Concolic Execution

`ConcolicState` evaluates expressions while collecting path constraints:

```rust
use concolic_pbt::ConcolicState;
use std::collections::HashMap;

let mut state = ConcolicState::new(HashMap::from([("x".to_string(), 3)]));
let result = state.eval(&expr);  // Returns 4
// state.constraints now contains: [(x <= 5, true)]
```

### Constraint Solver

`Solver` finds inputs satisfying constraints using random sampling with rejection:

- Extracts bounds from simple constraints (e.g., `x <= 5` → upper bound)
- Handles complex constraints (including `ite`) via rejection sampling
- No external SMT solver dependency

### Path Explorer

`Explorer` performs depth-first search over execution paths:

```rust
use concolic_pbt::{Explorer, Solver, ExploreResult, cmp, Expr};
use std::collections::HashMap;

let property = cmp!(Expr::var("x"), <=, Expr::lit(100));

let rng = rand::rngs::StdRng::seed_from_u64(42);
let solver = Solver::new(rng, 100);
let mut explorer = Explorer::new(solver, 1000);

match explorer.find_counterexample(&property, HashMap::from([("x".to_string(), 0)])) {
    ExploreResult::Counterexample(env) => println!("Found: {:?}", env),
    ExploreResult::Verified => println!("Property holds"),
    ExploreResult::MaxIterationsReached => println!("Inconclusive"),
}
```

## Scope

Implementing concolic execution for a real programming language is a significant undertaking. As a learning exercise, this project takes a simpler approach:

- Define a minimal programming language (roughly calculator-level complexity)
- Implement a concolic executor for this language
- Deliberately avoid loops (`while`), which are known to cause path explosion and termination issues in symbolic execution

## License

Copyright (c) 2026 Toshiki Teramura (@termoshtt)

This project is licensed under `MIT OR Apache-2.0`
