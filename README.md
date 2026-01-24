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

## Scope

Implementing concolic execution for a real programming language is a significant undertaking. As a learning exercise, this project takes a simpler approach:

- Define a minimal programming language (roughly calculator-level complexity)
- Implement a concolic executor for this language
- Deliberately avoid loops (`while`), which are known to cause path explosion and termination issues in symbolic execution

## License

Copyright (c) 2026 Toshiki Teramura (@termoshtt)

This project is licensed under `MIT OR Apache-2.0`
