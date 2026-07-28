//! Source text to tokens.
//!
//! Hand-rolled, no dependency: a parser crate would pull its own error
//! type into a `thiserror` library crate and add bytes to a wasm payload
//! that is already the product's largest download.
//!
//! `@name` is tokenized here but rejected by the expression parser. The
//! attribute scope belongs to the wrangle statement layer, and reserving
//! the syntax now means a user who types `@P` in a param field gets "the @
//! attribute scope is only available inside a wrangle" instead of a
//! baffling "unexpected character".

use super::ast::Var;
use super::error::ExprError;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    Number(f64),
    Str(String),
    Ident(String),
    Var(Var),
    /// `@name`, reserved for the wrangle.
    Attr(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    AndAnd,
    OrOr,
    Bang,
    Question,
    Colon,
    Semi,
    Assign,
    LParen,
    RParen,
    Comma,
    Dot,
}

impl Tok {
    /// How the token reads in an error message.
    pub(super) fn describe(&self) -> String {
        match self {
            Tok::Number(n) => format!("number {n}"),
            Tok::Str(s) => format!("string \"{s}\""),
            Tok::Ident(n) => format!("`{n}`"),
            Tok::Var(v) => format!("`${}`", v.name()),
            Tok::Attr(n) => format!("`@{n}`"),
            Tok::Plus => "`+`".into(),
            Tok::Minus => "`-`".into(),
            Tok::Star => "`*`".into(),
            Tok::Slash => "`/`".into(),
            Tok::Percent => "`%`".into(),
            Tok::Lt => "`<`".into(),
            Tok::Le => "`<=`".into(),
            Tok::Gt => "`>`".into(),
            Tok::Ge => "`>=`".into(),
            Tok::EqEq => "`==`".into(),
            Tok::Ne => "`!=`".into(),
            Tok::AndAnd => "`&&`".into(),
            Tok::OrOr => "`||`".into(),
            Tok::Bang => "`!`".into(),
            Tok::Question => "`?`".into(),
            Tok::Colon => "`:`".into(),
            Tok::Semi => "`;`".into(),
            Tok::Assign => "`=`".into(),
            Tok::LParen => "`(`".into(),
            Tok::RParen => "`)`".into(),
            Tok::Comma => "`,`".into(),
            Tok::Dot => "`.`".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Token {
    pub tok: Tok,
    pub span: Range<usize>,
}

/// Tokenizes the whole source.
///
/// Returns tokens in order; the parser is responsible for structure. An
/// unterminated string or an unknown character fails here with the span of
/// the offending byte.
pub(super) fn lex(src: &str) -> Result<Vec<Token>, ExprError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        let start = i;
        let c = bytes[i];

        // Whitespace, including the newlines a multi-line snippet carries.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comments, so a wrangle program can be annotated.
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Numbers: digits with an optional fraction and exponent. A
        // leading `.` is deliberately not a number, so `.x` stays
        // unambiguously a component selector.
        if c.is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len()
                && bytes[i] == b'.'
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mut j = i + 1;
                if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
                    j += 1;
                }
                if bytes.get(j).is_some_and(u8::is_ascii_digit) {
                    i = j;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text = &src[start..i];
            let value: f64 = text
                .parse()
                .map_err(|_| ExprError::new(format!("`{text}` is not a valid number"), start..i))?;
            out.push(Token {
                tok: Tok::Number(value),
                span: start..i,
            });
            continue;
        }

        // Identifiers and keywords.
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(src[start..i].to_string()),
                span: start..i,
            });
            continue;
        }

        // `$VAR`.
        if c == b'$' {
            i += 1;
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &src[name_start..i];
            if name.is_empty() {
                return Err(ExprError::new("`$` must be followed by a name", start..i));
            }
            let Some(var) = Var::from_name(name) else {
                return Err(ExprError::new(
                    format!("unknown variable `${name}`; expected $T, $F, $FPS, $PI or $E"),
                    start..i,
                ));
            };
            out.push(Token {
                tok: Tok::Var(var),
                span: start..i,
            });
            continue;
        }

        // `@attr`, reserved for the wrangle statement layer.
        if c == b'@' {
            i += 1;
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &src[name_start..i];
            if name.is_empty() {
                return Err(ExprError::new(
                    "`@` must be followed by an attribute name",
                    start..i,
                ));
            }
            out.push(Token {
                tok: Tok::Attr(name.to_string()),
                span: start..i,
            });
            continue;
        }

        // Strings, double-quoted, no escapes (paths and field names do not
        // need them, and every escape is a rule a user has to learn).
        if c == b'"' {
            i += 1;
            let text_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\n' {
                    return Err(ExprError::new(
                        "unterminated string: a string cannot span lines",
                        start..i,
                    ));
                }
                i += 1;
            }
            if i >= bytes.len() {
                return Err(ExprError::new("unterminated string", start..i));
            }
            let text = src[text_start..i].to_string();
            i += 1; // closing quote
            out.push(Token {
                tok: Tok::Str(text),
                span: start..i,
            });
            continue;
        }

        // Operators and punctuation, two-character forms first.
        let two = |a: u8, b: u8| c == a && bytes.get(i + 1) == Some(&b);
        let (tok, len) = if two(b'<', b'=') {
            (Tok::Le, 2)
        } else if two(b'>', b'=') {
            (Tok::Ge, 2)
        } else if two(b'=', b'=') {
            (Tok::EqEq, 2)
        } else if two(b'!', b'=') {
            (Tok::Ne, 2)
        } else if two(b'&', b'&') {
            (Tok::AndAnd, 2)
        } else if two(b'|', b'|') {
            (Tok::OrOr, 2)
        } else {
            let single = match c {
                b'+' => Tok::Plus,
                b'-' => Tok::Minus,
                b'*' => Tok::Star,
                b'/' => Tok::Slash,
                b'%' => Tok::Percent,
                b'<' => Tok::Lt,
                b'>' => Tok::Gt,
                b'!' => Tok::Bang,
                b'?' => Tok::Question,
                b':' => Tok::Colon,
                b';' => Tok::Semi,
                b'=' => Tok::Assign,
                b'(' => Tok::LParen,
                b')' => Tok::RParen,
                b',' => Tok::Comma,
                b'.' => Tok::Dot,
                b'&' => {
                    return Err(ExprError::new(
                        "`&` is not an operator; did you mean `&&`?",
                        start..start + 1,
                    ));
                }
                b'|' => {
                    return Err(ExprError::new(
                        "`|` is not an operator; did you mean `||`?",
                        start..start + 1,
                    ));
                }
                _ => {
                    let ch = src[start..].chars().next().unwrap_or('?');
                    return Err(ExprError::new(
                        format!("unexpected character `{ch}`"),
                        start..start + ch.len_utf8(),
                    ));
                }
            };
            (single, 1)
        };
        i += len;
        out.push(Token {
            tok,
            span: start..i,
        });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src)
            .expect("lexes")
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    #[test]
    fn numbers_cover_int_fraction_and_exponent() {
        assert_eq!(toks("1"), vec![Tok::Number(1.0)]);
        assert_eq!(toks("1.5"), vec![Tok::Number(1.5)]);
        assert_eq!(toks("2e3"), vec![Tok::Number(2000.0)]);
        assert_eq!(toks("2e-3"), vec![Tok::Number(0.002)]);
    }

    #[test]
    fn a_trailing_dot_is_a_separate_token_so_member_access_stays_unambiguous() {
        // `1.x` must not lex `1.` as a number, or `.x` selection breaks.
        assert_eq!(
            toks("1.x"),
            vec![Tok::Number(1.0), Tok::Dot, Tok::Ident("x".into())]
        );
    }

    #[test]
    fn two_character_operators_beat_their_prefixes() {
        assert_eq!(toks("<= >= == != && ||").len(), 6);
        assert_eq!(
            toks("a<=b"),
            vec![Tok::Ident("a".into()), Tok::Le, Tok::Ident("b".into())]
        );
        assert_eq!(
            toks("a<b"),
            vec![Tok::Ident("a".into()), Tok::Lt, Tok::Ident("b".into())]
        );
    }

    #[test]
    fn variables_are_recognised_and_unknown_ones_name_the_alternatives() {
        assert_eq!(toks("$T"), vec![Tok::Var(Var::Time)]);
        assert_eq!(toks("$PI"), vec![Tok::Var(Var::Pi)]);
        let err = lex("$Q").unwrap_err();
        assert!(err.message.contains("unknown variable `$Q`"), "{err:?}");
        assert_eq!(err.span, 0..2);
    }

    #[test]
    fn attributes_lex_but_are_the_parsers_problem() {
        assert_eq!(toks("@P"), vec![Tok::Attr("P".into())]);
        assert_eq!(toks("@my_lane"), vec![Tok::Attr("my_lane".into())]);
    }

    #[test]
    fn strings_carry_their_content_and_span_the_quotes() {
        let t = lex("\"a/b\"").expect("lexes");
        assert_eq!(t[0].tok, Tok::Str("a/b".into()));
        assert_eq!(t[0].span, 0..5);
    }

    #[test]
    fn an_unterminated_string_is_an_error_not_a_silent_truncation() {
        assert!(lex("\"abc").is_err());
        assert!(lex("\"abc\ndef\"").is_err());
    }

    #[test]
    fn comments_and_whitespace_are_skipped() {
        assert_eq!(
            toks("1 + // trailing\n2"),
            vec![Tok::Number(1.0), Tok::Plus, Tok::Number(2.0)]
        );
    }

    #[test]
    fn a_single_ampersand_suggests_the_real_operator() {
        let err = lex("a & b").unwrap_err();
        assert!(err.message.contains("&&"), "{err:?}");
    }

    #[test]
    fn an_unknown_character_reports_its_own_span_even_when_multibyte() {
        let err = lex("1 + ½").unwrap_err();
        assert!(err.message.contains('½'), "{err:?}");
        assert_eq!(err.span, 4..6, "a 2-byte char spans 2 bytes");
    }
}
