//! Tokens to a syntax tree, by precedence climbing.
//!
//! The grammar has no loops and no user-defined functions, so the sandbox
//! (decision M-23) is three numbers enforced here: source length, nesting
//! depth, and calls per expression. Depth is what stops
//! `((((((...))))))` from overflowing the stack during parsing *and*
//! during evaluation, since both walk the same tree.

use super::ast::{BinaryOp, Component, Expr, Parsed, UnaryOp};
use super::error::ExprError;
use super::lexer::{Tok, Token, lex};

/// Maximum source length in bytes.
pub const MAX_SOURCE_LEN: usize = 4096;
/// Maximum parser recursion depth.
///
/// This counts stack frames, not parentheses: descending through one
/// bracketed group costs about four frames (expression, unary, postfix,
/// primary), so this budget is roughly 60 levels of real nesting. It is
/// stated in frames because that is what it actually protects, the parse
/// stack, and a limit that claims to count something else is a limit
/// nobody can predict.
pub const MAX_DEPTH: usize = 256;
/// Maximum number of function calls in one expression.
pub const MAX_CALLS: usize = 256;

/// Binding powers, lowest first. Ternary is handled separately because it
/// is right-associative and has two operands after the condition.
fn binary_power(tok: &Tok) -> Option<(BinaryOp, u8)> {
    let (op, power) = match tok {
        Tok::OrOr => (BinaryOp::Or, 1),
        Tok::AndAnd => (BinaryOp::And, 2),
        Tok::EqEq => (BinaryOp::Eq, 3),
        Tok::Ne => (BinaryOp::Ne, 3),
        Tok::Lt => (BinaryOp::Lt, 4),
        Tok::Le => (BinaryOp::Le, 4),
        Tok::Gt => (BinaryOp::Gt, 4),
        Tok::Ge => (BinaryOp::Ge, 4),
        Tok::Plus => (BinaryOp::Add, 5),
        Tok::Minus => (BinaryOp::Sub, 5),
        Tok::Star => (BinaryOp::Mul, 6),
        Tok::Slash => (BinaryOp::Div, 6),
        Tok::Percent => (BinaryOp::Rem, 6),
        _ => return None,
    };
    Some((op, power))
}

/// How the parser treats `@name` and bare identifiers.
///
/// The two modes share the whole precedence-climbing parser; they differ
/// only in what a primary may be. That is deliberate: the wrangle's
/// expressions ARE parameter expressions plus an element scope, and forking
/// the grammar would be two grammars to keep in step.
pub(super) trait Scope {
    /// Resolves `@name` to a lane slot, or `None` when `@` is illegal here.
    fn attr(&mut self, name: &str) -> Option<usize>;
    /// Resolves a bare identifier to a local slot, or `None` when bare
    /// identifiers are illegal here.
    fn local(&mut self, name: &str) -> Option<usize>;
}

/// The parameter-expression scope: no element data of any kind.
pub(super) struct NoScope;

impl Scope for NoScope {
    fn attr(&mut self, _name: &str) -> Option<usize> {
        None
    }
    fn local(&mut self, _name: &str) -> Option<usize> {
        None
    }
}

struct Parser<'s> {
    tokens: Vec<Token>,
    pos: usize,
    src_len: usize,
    calls: usize,
    uses_time: bool,
    scope: &'s mut dyn Scope,
}

/// Parses one expression, rejecting anything left over.
pub fn parse(source: &str) -> Result<Parsed, ExprError> {
    parse_scoped(source, &mut NoScope)
}

/// Parses one expression under `scope`, which decides whether `@name` and
/// bare identifiers resolve. The statement layer uses this to reuse the
/// whole grammar; [`parse`] is the same call with everything refused.
pub(super) fn parse_scoped(source: &str, scope: &mut dyn Scope) -> Result<Parsed, ExprError> {
    if source.len() > MAX_SOURCE_LEN {
        return Err(ExprError::new(
            format!(
                "expression is {} bytes; the limit is {MAX_SOURCE_LEN}",
                source.len()
            ),
            0..source.len().min(MAX_SOURCE_LEN),
        ));
    }
    let tokens = lex(source)?;
    if tokens.is_empty() {
        return Err(ExprError::new("the expression is empty", 0..source.len()));
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        src_len: source.len(),
        calls: 0,
        uses_time: false,
        scope,
    };
    let root = p.expr(0, 0)?;
    if let Some(extra) = p.peek() {
        let span = extra.span.clone();
        let what = extra.tok.describe();
        return Err(ExprError::new(format!("unexpected {what}"), span));
    }
    Ok(Parsed {
        root,
        uses_time: p.uses_time,
    })
}

/// Parses an expression from an already-lexed token run, so the statement
/// parser can hand over a sub-slice without re-lexing.
pub(super) fn parse_tokens(
    tokens: Vec<Token>,
    src_len: usize,
    scope: &mut dyn Scope,
) -> Result<Parsed, ExprError> {
    if tokens.is_empty() {
        return Err(ExprError::new("expected a value", 0..src_len));
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        src_len,
        calls: 0,
        uses_time: false,
        scope,
    };
    let root = p.expr(0, 0)?;
    if let Some(extra) = p.peek() {
        let span = extra.span.clone();
        let what = extra.tok.describe();
        return Err(ExprError::new(format!("unexpected {what}"), span));
    }
    Ok(Parsed {
        root,
        uses_time: p.uses_time,
    })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// The span to blame when input ran out.
    fn eof_span(&self) -> std::ops::Range<usize> {
        self.tokens.last().map_or(0..self.src_len, |t| {
            t.span.end..self.src_len.max(t.span.end)
        })
    }

    fn expect(&mut self, want: &Tok, context: &str) -> Result<Token, ExprError> {
        match self.next() {
            Some(t) if t.tok == *want => Ok(t),
            Some(t) => {
                let what = t.tok.describe();
                Err(ExprError::new(
                    format!("expected {} {context}, found {what}", want.describe()),
                    t.span,
                ))
            }
            None => Err(ExprError::new(
                format!("expected {} {context}", want.describe()),
                self.eof_span(),
            )),
        }
    }

    fn check_depth(depth: usize, span: std::ops::Range<usize>) -> Result<(), ExprError> {
        if depth > MAX_DEPTH {
            return Err(ExprError::new("expression nests too deeply", span));
        }
        Ok(())
    }

    /// The span of the next token, for a depth error that has to blame
    /// something.
    fn here(&self) -> std::ops::Range<usize> {
        self.peek().map_or(0..self.src_len, |t| t.span.clone())
    }

    /// Precedence climbing. `min_power` is the lowest binding power this
    /// call will consume.
    fn expr(&mut self, min_power: u8, depth: usize) -> Result<Expr, ExprError> {
        Self::check_depth(depth, self.here())?;
        let mut lhs = self.unary(depth + 1)?;

        // The token kind is copied out rather than borrowed, so the body
        // stays free to advance `self.pos`. Only the operator variants get
        // this far; anything that allocates breaks out immediately.
        while let Some(kind) = self.peek().map(|t| t.tok.clone()) {
            // Ternary binds looser than every binary operator and is
            // right-associative, so it is folded in here at power 0.
            if kind == Tok::Question {
                if min_power > 0 {
                    break;
                }
                self.pos += 1;
                let then = self.expr(0, depth + 1)?;
                self.expect(&Tok::Colon, "in a `? :` conditional")?;
                let otherwise = self.expr(0, depth + 1)?;
                lhs = Expr::Ternary {
                    cond: Box::new(lhs),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                };
                continue;
            }
            let Some((op, power)) = binary_power(&kind) else {
                break;
            };
            if power < min_power {
                break;
            }
            self.pos += 1;
            // Left-associative: the right side may only take strictly
            // tighter operators.
            let rhs = self.expr(power + 1, depth + 1)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self, depth: usize) -> Result<Expr, ExprError> {
        Self::check_depth(depth, self.here())?;
        match self.peek().map(|t| t.tok.clone()) {
            Some(Tok::Minus) => {
                self.pos += 1;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    rhs: Box::new(self.unary(depth + 1)?),
                })
            }
            Some(Tok::Bang) => {
                self.pos += 1;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    rhs: Box::new(self.unary(depth + 1)?),
                })
            }
            // Unary plus is accepted and folds away: `+1` is what a user
            // writes when lining up a column of signed numbers.
            Some(Tok::Plus) => {
                self.pos += 1;
                self.unary(depth + 1)
            }
            _ => self.postfix(depth + 1),
        }
    }

    /// Primary followed by any number of `.component` selectors.
    fn postfix(&mut self, depth: usize) -> Result<Expr, ExprError> {
        let mut base = self.primary(depth)?;
        while self.peek().is_some_and(|t| t.tok == Tok::Dot) {
            let dot = self.eof_span();
            self.pos += 1;
            let Some(tok) = self.next() else {
                return Err(ExprError::new(
                    "expected a component (`x`, `y`, `z` or `w`) after `.`",
                    dot,
                ));
            };
            let Tok::Ident(name) = &tok.tok else {
                let what = tok.tok.describe();
                return Err(ExprError::new(
                    format!("expected a component after `.`, found {what}"),
                    tok.span,
                ));
            };
            let Some(component) = Component::from_name(name) else {
                return Err(ExprError::new(
                    format!("`{name}` is not a component; use `x`, `y`, `z` or `w`"),
                    tok.span,
                ));
            };
            base = Expr::Member {
                base: Box::new(base),
                component,
            };
        }
        Ok(base)
    }

    fn primary(&mut self, depth: usize) -> Result<Expr, ExprError> {
        Self::check_depth(depth, self.here())?;
        let Some(token) = self.next() else {
            return Err(ExprError::new("expected a value", self.eof_span()));
        };
        let span = token.span.clone();
        match token.tok {
            Tok::Number(v) => Ok(Expr::Number(v)),
            Tok::Var(v) => {
                if v.is_time() {
                    self.uses_time = true;
                }
                Ok(Expr::Var(v))
            }
            // A bare string is not a value: there is no string type in the
            // lattice (M-3), so it is only ever a call argument.
            Tok::Str(_) => Err(ExprError::new(
                "a string is only valid as an argument to ch() or bbox()",
                span,
            )),
            Tok::Attr(name) => match self.scope.attr(&name) {
                Some(slot) => Ok(Expr::Attr(slot)),
                None => Err(ExprError::new(
                    format!(
                        "`@{name}`: the @ attribute scope is only available inside a wrangle, \
                         not in a parameter expression"
                    ),
                    span,
                )),
            },
            Tok::LParen => {
                let inner = self.expr(0, depth + 1)?;
                self.expect(&Tok::RParen, "to close the group")?;
                Ok(inner)
            }
            Tok::Ident(name) => {
                if self.peek().is_some_and(|t| t.tok == Tok::LParen) {
                    self.call(name, span, depth)
                } else if let Some(slot) = self.scope.local(&name) {
                    // Inside a wrangle a bare name may be a local declared
                    // earlier in the same program. An undeclared one still
                    // falls through to the error below, so a typo names
                    // itself rather than reading as zero.
                    Ok(Expr::Local(slot))
                } else {
                    // No bare identifiers: every name is either a call or a
                    // `$variable`, so a typo names itself rather than
                    // silently reading as zero.
                    Err(ExprError::new(
                        format!(
                            "`{name}` is not a value; call it as `{name}(...)` \
                             or use a `$variable`"
                        ),
                        span,
                    ))
                }
            }
            other => {
                let what = other.describe();
                Err(ExprError::new(
                    format!("expected a value, found {what}"),
                    span,
                ))
            }
        }
    }

    fn call(
        &mut self,
        name: String,
        name_span: std::ops::Range<usize>,
        depth: usize,
    ) -> Result<Expr, ExprError> {
        self.calls += 1;
        if self.calls > MAX_CALLS {
            return Err(ExprError::new(
                format!("expression makes more than {MAX_CALLS} calls"),
                name_span,
            ));
        }
        self.expect(&Tok::LParen, "to open the argument list")?;
        let mut args = Vec::new();
        if self.peek().is_some_and(|t| t.tok == Tok::RParen) {
            let close = self.expect(&Tok::RParen, "to close the argument list")?;
            return Ok(Expr::Call {
                name,
                args,
                span: name_span.start..close.span.end,
            });
        }
        loop {
            // A string argument is taken verbatim; anything else is a
            // full expression.
            if let Some(Token {
                tok: Tok::Str(s), ..
            }) = self.peek().cloned()
            {
                self.pos += 1;
                args.push(Expr::Str(s));
            } else {
                args.push(self.expr(0, depth + 1)?);
            }
            match self.peek().map(|t| t.tok.clone()) {
                Some(Tok::Comma) => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let close = self.expect(&Tok::RParen, "to close the argument list")?;
        Ok(Expr::Call {
            name,
            args,
            span: name_span.start..close.span.end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Expr {
        parse(src).expect("parses").root
    }

    fn err(src: &str) -> ExprError {
        parse(src).expect_err("should fail")
    }

    fn num(v: f64) -> Expr {
        Expr::Number(v)
    }

    fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op,
            lhs: Box::new(l),
            rhs: Box::new(r),
        }
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_eq!(
            p("1 + 2 * 3"),
            bin(
                BinaryOp::Add,
                num(1.0),
                bin(BinaryOp::Mul, num(2.0), num(3.0))
            )
        );
    }

    #[test]
    fn subtraction_is_left_associative() {
        // (1 - 2) - 3, not 1 - (2 - 3): the difference is a sign error.
        assert_eq!(
            p("1 - 2 - 3"),
            bin(
                BinaryOp::Sub,
                bin(BinaryOp::Sub, num(1.0), num(2.0)),
                num(3.0)
            )
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        assert_eq!(
            p("(1 + 2) * 3"),
            bin(
                BinaryOp::Mul,
                bin(BinaryOp::Add, num(1.0), num(2.0)),
                num(3.0)
            )
        );
    }

    #[test]
    fn comparison_binds_looser_than_arithmetic_and_tighter_than_logic() {
        assert_eq!(
            p("1 + 1 < 3 && 1 > 0"),
            bin(
                BinaryOp::And,
                bin(
                    BinaryOp::Lt,
                    bin(BinaryOp::Add, num(1.0), num(1.0)),
                    num(3.0)
                ),
                bin(BinaryOp::Gt, num(1.0), num(0.0)),
            )
        );
    }

    #[test]
    fn ternary_is_right_associative_and_looser_than_everything() {
        // a ? b : (c ? d : e)
        let parsed = p("1 ? 2 : 3 ? 4 : 5");
        let Expr::Ternary { otherwise, .. } = parsed else {
            panic!("expected a ternary");
        };
        assert!(matches!(*otherwise, Expr::Ternary { .. }));
    }

    #[test]
    fn unary_minus_binds_tighter_than_multiplication() {
        assert_eq!(
            p("-2 * 3"),
            bin(
                BinaryOp::Mul,
                Expr::Unary {
                    op: UnaryOp::Neg,
                    rhs: Box::new(num(2.0))
                },
                num(3.0)
            )
        );
    }

    #[test]
    fn member_access_chains_and_binds_tightest() {
        let parsed = p("set(1,2,3).x * 2");
        let Expr::Binary { lhs, .. } = parsed else {
            panic!("expected a product");
        };
        assert!(matches!(*lhs, Expr::Member { .. }));
    }

    #[test]
    fn calls_take_expressions_and_strings() {
        let parsed = p("ch(\"../a/b\")");
        let Expr::Call { name, args, .. } = parsed else {
            panic!("expected a call");
        };
        assert_eq!(name, "ch");
        assert_eq!(args, vec![Expr::Str("../a/b".into())]);
    }

    #[test]
    fn a_call_may_be_empty_or_nested() {
        assert!(matches!(p("npoints()"), Expr::Call { .. }));
        assert!(matches!(p("max(1, min(2, 3))"), Expr::Call { .. }));
    }

    #[test]
    fn uses_time_is_set_only_by_time_variables() {
        assert!(parse("$T * 2").expect("parses").uses_time);
        assert!(parse("$F").expect("parses").uses_time);
        assert!(!parse("$PI * 2").expect("parses").uses_time);
        assert!(!parse("1 + 2").expect("parses").uses_time);
        // Nested inside a call still counts, or the runtime would skip the node.
        assert!(parse("sin(max($T, 0))").expect("parses").uses_time);
    }

    #[test]
    fn trailing_input_is_an_error_with_its_own_span() {
        let e = err("1 + 2 3");
        assert!(e.message.contains("unexpected"), "{e:?}");
        assert_eq!(e.span, 6..7);
    }

    #[test]
    fn an_unclosed_group_names_what_it_wanted() {
        let e = err("(1 + 2");
        assert!(e.message.contains("`)`"), "{e:?}");
    }

    #[test]
    fn a_bare_identifier_suggests_the_call_form() {
        let e = err("radius");
        assert!(e.message.contains("radius(...)"), "{e:?}");
    }

    #[test]
    fn a_bare_string_is_refused_because_there_is_no_string_type() {
        let e = err("\"hello\"");
        assert!(e.message.contains("only valid as an argument"), "{e:?}");
    }

    #[test]
    fn an_attribute_in_a_param_expression_explains_where_it_belongs() {
        let e = err("@P.y");
        assert!(
            e.message.contains("only available inside a wrangle"),
            "{e:?}"
        );
    }

    #[test]
    fn a_bad_component_lists_the_real_ones() {
        let e = err("set(1,2,3).q");
        assert!(e.message.contains("`x`, `y`, `z` or `w`"), "{e:?}");
    }

    #[test]
    fn an_empty_expression_is_an_error_not_a_zero() {
        assert!(err("").message.contains("empty"));
        assert!(err("   ").message.contains("empty"));
    }

    // The M-23 sandbox. The grammar has no loops, so these three limits
    // are the whole of it.

    #[test]
    fn source_longer_than_the_limit_is_refused_before_lexing() {
        let src = "1+".repeat(MAX_SOURCE_LEN);
        let e = parse(&src).expect_err("too long");
        assert!(e.message.contains("the limit is"), "{e:?}");
    }

    #[test]
    fn nesting_deeper_than_the_limit_is_refused_without_overflowing() {
        // Well past the frame budget: this is the input that would blow the
        // parse stack if nothing counted.
        let deep = format!("{}1{}", "(".repeat(400), ")".repeat(400));
        let e = parse(&deep).expect_err("too deep");
        assert!(e.message.contains("nests too deeply"), "{e:?}");
    }

    #[test]
    fn the_frame_budget_allows_at_least_fifty_levels_of_real_nesting() {
        // The budget is stated in stack frames, so this pins the nesting
        // depth a user actually gets: comfortably more than anyone writes.
        let ok = format!("{}1{}", "(".repeat(50), ")".repeat(50));
        assert!(parse(&ok).is_ok(), "50 levels of parentheses must parse");
    }

    #[test]
    fn more_calls_than_the_limit_are_refused() {
        // MAX_CALLS + 1 sibling calls, nested shallowly enough to pass depth.
        let calls = vec!["abs(1)"; MAX_CALLS + 1].join(" + ");
        let e = parse(&calls).expect_err("too many calls");
        assert!(e.message.contains("more than"), "{e:?}");
    }
}
