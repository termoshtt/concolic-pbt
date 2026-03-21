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

#### Grammar

```text
expr       := if_expr | arith_expr
if_expr    := "if" bool_expr "then" expr "else" expr
arith_expr := term (('+' | '-') term)*
term       := number | var | '(' expr ')'

bool_expr  := "true" | "false" | expr cmp_op expr
cmp_op     := "<=" | ">=" | "=="

var        := [a-z][a-z0-9_]*
number     := '-'? [0-9]+
```

#### Example

```rust
use concolic_pbt::parse_expr;

let expr = parse_expr("if x <= 5 then x + 1 else x - 1").unwrap();
```

### Concolic Execution

`ConcolicState` evaluates expressions while collecting path constraints:

```rust
use concolic_pbt::{parse_expr, ConcolicState};
use std::collections::HashMap;

let expr = parse_expr("if x <= 5 then x + 1 else x - 1").unwrap();
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

`Explorer` performs depth-first search over execution paths to find counterexamples.

The core goal of this project: given a property with conditional branches, automatically find an input that makes it false.

```rust
use concolic_pbt::{parse_bool_expr, Explorer, Solver, Stmt, Stmts, ExploreResult};
use rand::SeedableRng;
use std::collections::HashMap;

// Property: (if x <= 5 then x + 1 else x - 1) <= 10
// This should hold for most inputs, but fails when x > 11
// (because x - 1 > 10 when x > 11)
let property = parse_bool_expr("(if x <= 5 then x + 1 else x - 1) <= 10").unwrap();
let stmts = Stmts(vec![Stmt::assert(property)]);

let rng = rand::rngs::StdRng::seed_from_u64(42);
let solver = Solver::new(rng, 100);
let mut explorer = Explorer::new(solver, 1000);

// Start with x = 3 (takes the then-branch, satisfies property)
// Explorer will automatically explore the else-branch and find x > 11
match explorer.find_counterexample(&stmts, HashMap::from([("x".to_string(), 3)])) {
    ExploreResult::Counterexample { env, failure } => {
        // Found: x = 149 (or some value > 11)
        // failure is AssertionFailed for the property
        println!("Counterexample: {:?}, failure: {:?}", env, failure);
    }
    ExploreResult::Verified => println!("Property holds for all paths"),
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
