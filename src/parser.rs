use chumsky::prelude::*;

use crate::{BoolExpr, Expr};

/// Parser for the expression language
///
/// Grammar:
/// ```text
/// expr       := if_expr | arith_expr
/// if_expr    := "if" bool_expr "then" expr "else" expr
/// arith_expr := term (('+' | '-') term)*
/// term       := number | var | '(' expr ')'
///
/// bool_expr  := "true" | "false" | expr cmp_op expr
/// cmp_op     := "<=" | ">=" | "=="
///
/// var        := [a-z][a-z0-9_]*
/// number     := '-'? [0-9]+
/// ```
fn parser<'a>() -> impl Parser<'a, &'a str, Expr, extra::Err<Rich<'a, char>>> {
    recursive(|expr| {
        // Number literal: optional minus followed by digits
        let number = just('-')
            .or_not()
            .then(text::int(10))
            .map(|(neg, s): (Option<_>, &str)| {
                let n: i64 = s.parse().unwrap();
                Expr::Lit(if neg.is_some() { -n } else { n })
            })
            .padded();

        // Variable: lowercase letter followed by alphanumeric/underscore
        // Keywords are excluded by checking against reserved words
        let var = text::ident()
            .try_map(|s: &str, span| {
                let first = s.chars().next().unwrap();
                if first.is_ascii_lowercase() && !["if", "then", "else", "true", "false"].contains(&s) {
                    Ok(Expr::Var(s.to_string()))
                } else {
                    Err(Rich::custom(span, format!("'{}' is not a valid variable name", s)))
                }
            })
            .padded();

        // Parenthesized expression
        let paren = expr
            .clone()
            .delimited_by(just('(').padded(), just(')').padded());

        // Atomic expression (number, variable, or parenthesized)
        let atom = number.or(paren).or(var);

        // Arithmetic: term (('+' | '-') term)*
        let arith = atom.clone().foldl(
            choice((just('+').to(true), just('-').to(false)))
                .padded()
                .then(atom)
                .repeated(),
            |lhs, (is_add, rhs)| {
                if is_add {
                    Expr::Add(Box::new(lhs), Box::new(rhs))
                } else {
                    Expr::Sub(Box::new(lhs), Box::new(rhs))
                }
            },
        );

        // Comparison operators
        let cmp_op = choice((
            just("<=").to(BoolOp::Le),
            just(">=").to(BoolOp::Ge),
            just("==").to(BoolOp::Eq),
        ))
        .padded();

        // Boolean expression: "true" | "false" | expr cmp_op expr
        let bool_lit = choice((
            text::keyword("true").to(BoolExpr::Lit(true)),
            text::keyword("false").to(BoolExpr::Lit(false)),
        ))
        .padded();

        let bool_cmp = arith
            .clone()
            .then(cmp_op)
            .then(arith.clone())
            .map(|((lhs, op), rhs)| match op {
                BoolOp::Le => BoolExpr::Le(Box::new(lhs), Box::new(rhs)),
                BoolOp::Ge => BoolExpr::Ge(Box::new(lhs), Box::new(rhs)),
                BoolOp::Eq => BoolExpr::Eq(Box::new(lhs), Box::new(rhs)),
            });

        let bool_expr = bool_lit.or(bool_cmp);

        // If expression: "if" bool_expr "then" expr "else" expr
        let if_expr = text::keyword("if")
            .padded()
            .ignore_then(bool_expr)
            .then_ignore(text::keyword("then").padded())
            .then(expr.clone())
            .then_ignore(text::keyword("else").padded())
            .then(expr)
            .map(|((cond, then_), else_)| {
                Expr::If(Box::new(cond), Box::new(then_), Box::new(else_))
            });

        if_expr.or(arith)
    })
}

#[derive(Clone, Copy)]
enum BoolOp {
    Le,
    Ge,
    Eq,
}

/// Parse an expression from a string
pub fn parse_expr(input: &str) -> Result<Expr, Vec<Rich<'_, char>>> {
    parser().parse(input).into_result()
}

/// Parse a boolean expression from a string
pub fn parse_bool_expr(input: &str) -> Result<BoolExpr, Vec<Rich<'_, char>>> {
    bool_parser().parse(input).into_result()
}

fn bool_parser<'a>() -> impl Parser<'a, &'a str, BoolExpr, extra::Err<Rich<'a, char>>> {
    recursive(|_bool_expr| {
        // Reuse the expr parser for the comparison operands
        let expr = recursive(|expr| {
            let number = just('-')
                .or_not()
                .then(text::int(10))
                .map(|(neg, s): (Option<_>, &str)| {
                    let n: i64 = s.parse().unwrap();
                    Expr::Lit(if neg.is_some() { -n } else { n })
                })
                .padded();

            let var = text::ident()
                .try_map(|s: &str, span| {
                    let first = s.chars().next().unwrap();
                    if first.is_ascii_lowercase() && !["if", "then", "else", "true", "false"].contains(&s) {
                        Ok(Expr::Var(s.to_string()))
                    } else {
                        Err(Rich::custom(span, format!("'{}' is not a valid variable name", s)))
                    }
                })
                .padded();

            let paren = expr
                .clone()
                .delimited_by(just('(').padded(), just(')').padded());

            let atom = number.or(paren).or(var);

            atom.clone().foldl(
                choice((just('+').to(true), just('-').to(false)))
                    .padded()
                    .then(atom)
                    .repeated(),
                |lhs, (is_add, rhs)| {
                    if is_add {
                        Expr::Add(Box::new(lhs), Box::new(rhs))
                    } else {
                        Expr::Sub(Box::new(lhs), Box::new(rhs))
                    }
                },
            )
        });

        let bool_lit = choice((
            text::keyword("true").to(BoolExpr::Lit(true)),
            text::keyword("false").to(BoolExpr::Lit(false)),
        ))
        .padded();

        let cmp_op = choice((
            just("<=").to(BoolOp::Le),
            just(">=").to(BoolOp::Ge),
            just("==").to(BoolOp::Eq),
        ))
        .padded();

        let bool_cmp = expr
            .clone()
            .then(cmp_op)
            .then(expr)
            .map(|((lhs, op), rhs)| match op {
                BoolOp::Le => BoolExpr::Le(Box::new(lhs), Box::new(rhs)),
                BoolOp::Ge => BoolExpr::Ge(Box::new(lhs), Box::new(rhs)),
                BoolOp::Eq => BoolExpr::Eq(Box::new(lhs), Box::new(rhs)),
            });

        bool_lit.or(bool_cmp)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_number() {
        assert_eq!(parse_expr("42").unwrap(), Expr::Lit(42));
        assert_eq!(parse_expr("-5").unwrap(), Expr::Lit(-5));
        assert_eq!(parse_expr(" 123 ").unwrap(), Expr::Lit(123));
    }

    #[test]
    fn parse_var() {
        assert_eq!(parse_expr("x").unwrap(), Expr::Var("x".to_string()));
        assert_eq!(parse_expr("foo").unwrap(), Expr::Var("foo".to_string()));
        assert_eq!(
            parse_expr("var123").unwrap(),
            Expr::Var("var123".to_string())
        );
    }

    #[test]
    fn parse_arithmetic() {
        let x = || Expr::Var("x".to_string());

        assert_eq!(
            parse_expr("x + 1").unwrap(),
            Expr::Add(Box::new(x()), Box::new(Expr::Lit(1)))
        );
        assert_eq!(
            parse_expr("x - 1").unwrap(),
            Expr::Sub(Box::new(x()), Box::new(Expr::Lit(1)))
        );
        assert_eq!(
            parse_expr("x + 1 - 2").unwrap(),
            Expr::Sub(
                Box::new(Expr::Add(Box::new(x()), Box::new(Expr::Lit(1)))),
                Box::new(Expr::Lit(2))
            )
        );
    }

    #[test]
    fn parse_parens() {
        assert_eq!(parse_expr("(42)").unwrap(), Expr::Lit(42));
        assert_eq!(
            parse_expr("(x + 1)").unwrap(),
            Expr::Add(
                Box::new(Expr::Var("x".to_string())),
                Box::new(Expr::Lit(1))
            )
        );
    }

    #[test]
    fn parse_bool_literals() {
        assert_eq!(parse_bool_expr("true").unwrap(), BoolExpr::Lit(true));
        assert_eq!(parse_bool_expr("false").unwrap(), BoolExpr::Lit(false));
    }

    #[test]
    fn parse_comparisons() {
        let x = || Expr::Var("x".to_string());

        assert_eq!(
            parse_bool_expr("x <= 5").unwrap(),
            BoolExpr::Le(Box::new(x()), Box::new(Expr::Lit(5)))
        );
        assert_eq!(
            parse_bool_expr("x >= 10").unwrap(),
            BoolExpr::Ge(Box::new(x()), Box::new(Expr::Lit(10)))
        );
        assert_eq!(
            parse_bool_expr("x == 0").unwrap(),
            BoolExpr::Eq(Box::new(x()), Box::new(Expr::Lit(0)))
        );
    }

    #[test]
    fn parse_if_expr() {
        let result = parse_expr("if x <= 5 then 1 else 0").unwrap();
        assert_eq!(
            result,
            Expr::If(
                Box::new(BoolExpr::Le(
                    Box::new(Expr::Var("x".to_string())),
                    Box::new(Expr::Lit(5))
                )),
                Box::new(Expr::Lit(1)),
                Box::new(Expr::Lit(0))
            )
        );
    }

    #[test]
    fn parse_nested_if() {
        let result = parse_expr("if x <= 5 then if x >= 0 then x else 0 else 10").unwrap();
        assert_eq!(
            result,
            Expr::If(
                Box::new(BoolExpr::Le(
                    Box::new(Expr::Var("x".to_string())),
                    Box::new(Expr::Lit(5))
                )),
                Box::new(Expr::If(
                    Box::new(BoolExpr::Ge(
                        Box::new(Expr::Var("x".to_string())),
                        Box::new(Expr::Lit(0))
                    )),
                    Box::new(Expr::Var("x".to_string())),
                    Box::new(Expr::Lit(0))
                )),
                Box::new(Expr::Lit(10))
            )
        );
    }

    #[test]
    fn parse_complex_expr() {
        let result = parse_expr("if x + 1 <= 10 then x - 1 else 0").unwrap();
        assert_eq!(
            result,
            Expr::If(
                Box::new(BoolExpr::Le(
                    Box::new(Expr::Add(
                        Box::new(Expr::Var("x".to_string())),
                        Box::new(Expr::Lit(1))
                    )),
                    Box::new(Expr::Lit(10))
                )),
                Box::new(Expr::Sub(
                    Box::new(Expr::Var("x".to_string())),
                    Box::new(Expr::Lit(1))
                )),
                Box::new(Expr::Lit(0))
            )
        );
    }
}
