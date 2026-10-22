//! Validate before compilation: fuel/output bounds do not bound VM values.
use super::invalid;
use crate::Result;
use minijinja::machinery::{
    Token, WhitespaceConfig, ast::BinOpKind, ast::Expr, ast::Stmt, parse, tokenize,
};
use minijinja::syntax::SyntaxConfig;

pub(super) fn validate(template: &str) -> Result<()> {
    if template.len() > 256 * 1024 {
        return Err(invalid("template too large"));
    }
    // Lex first: the parser itself can fold constant multiplication/concat.
    // Unlike a delimiter regex, the lexer handles quotes/comments/raw blocks.
    for (n, token) in
        tokenize(template, false, SyntaxConfig, WhitespaceConfig::default()).enumerate()
    {
        let (token, _) = token.map_err(|e| invalid(e.to_string()))?;
        if n >= 4_096
            || matches!(
                token,
                Token::Mul | Token::Pow | Token::Plus | Token::Tilde | Token::Assign
            )
        {
            return Err(invalid(
                "template allocation operators/assignment or token cap",
            ));
        }
    }
    let parsed = parse(
        template,
        "document",
        SyntaxConfig,
        WhitespaceConfig::default(),
    )
    .map_err(|e| invalid(e.to_string()))?;
    statement(&parsed)
}

fn expression(expr: &Expr<'_>) -> Result<()> {
    match expr {
        Expr::Var(_) | Expr::Const(_) => Ok(()),
        Expr::GetAttr(e) => expression(&e.expr),
        Expr::GetItem(e) => {
            expression(&e.expr)?;
            expression(&e.subscript_expr)
        }
        Expr::Compare(e) => {
            expression(&e.expr)?;
            for op in &e.ops {
                expression(&op.expr)?;
            }
            Ok(())
        }
        Expr::BinOp(e)
            if matches!(
                e.op,
                BinOpKind::Eq
                    | BinOpKind::Ne
                    | BinOpKind::Lt
                    | BinOpKind::Lte
                    | BinOpKind::Gt
                    | BinOpKind::Gte
                    | BinOpKind::In
                    | BinOpKind::ScAnd
                    | BinOpKind::ScOr
            ) =>
        {
            expression(&e.left)?;
            expression(&e.right)
        }
        Expr::Filter(e) if matches!(e.name, "upper" | "lower") && e.args.is_empty() => expression(
            e.expr
                .as_ref()
                .ok_or_else(|| invalid("filter needs a value"))?,
        ),
        _ => Err(invalid(
            "unsupported template expression; use fields, indexes and upper/lower",
        )),
    }
}

fn statements(body: &[Stmt<'_>]) -> Result<()> {
    for stmt in body {
        statement(stmt)?;
    }
    Ok(())
}

fn statement(stmt: &Stmt<'_>) -> Result<()> {
    match stmt {
        Stmt::Template(t) => statements(&t.children),
        Stmt::EmitRaw(_) => Ok(()),
        Stmt::EmitExpr(e) => expression(&e.expr),
        Stmt::ForLoop(f) if !f.recursive && matches!(f.target, Expr::Var(_)) => {
            expression(&f.iter)?;
            if let Some(e) = &f.filter_expr {
                expression(e)?;
            }
            statements(&f.body)?;
            statements(&f.else_body)
        }
        Stmt::IfCond(c) => {
            expression(&c.expr)?;
            statements(&c.true_body)?;
            statements(&c.false_body)
        }
        _ => Err(invalid(
            "unsupported template statement; use for/if without capture or assignment",
        )),
    }
}
