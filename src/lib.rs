#![doc = include_str!("../README.md")]

mod explore;
mod node;
mod parser;
mod solver;
mod state;

pub use explore::{ExploreResult, Explorer};
pub use node::{Ast, BoolExpr, Env, Expr, SsaVar, Stage, Stmt, Stmts, Symbolic};
pub use parser::{parse_bool_expr, parse_expr, parse_stmts};
pub use solver::{Bound, Bounds, Solver, SolverError, extract_bounds, negate_at};
pub use state::{exec, ExecutionTrace, OracleFailure};
