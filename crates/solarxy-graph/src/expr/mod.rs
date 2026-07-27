//! The expression language.
//!
//! A parameter is either a literal or an expression
//! ([`crate::params::ParamSource`]), and this module is what turns the
//! second into a value. The seam it drops into is
//! [`crate::registry::resolve`]: an evaluated result flows through exactly
//! the same conform, clamp and unit path a literal does, so an expression
//! cannot smuggle an out-of-range or wrongly-typed value past the
//! resolver.
//!
//! Scope and its reasons:
//!
//! - **Hand-rolled, no parser dependency.** A parser crate would pull its
//!   own error type into a `thiserror` library crate and add bytes to the
//!   wasm payload, which is the product's largest download.
//! - **No string type in the value lattice** (decision M-3). Strings exist
//!   only as literal arguments to `ch()` and `bbox()`, so every evaluated
//!   value lands in the numeric union the resolver already conforms.
//! - **No loops, no user-defined functions.** That is what makes the
//!   sandbox three numbers ([`parser::MAX_SOURCE_LEN`],
//!   [`parser::MAX_DEPTH`], [`parser::MAX_CALLS`]) rather than a runtime
//!   budget (decision M-23).
//!
//! `@name` is lexed but refused by the expression parser: the attribute
//! scope belongs to the wrangle's statement layer, and reserving the
//! syntax means typing `@P` into a parameter field explains itself.

pub mod ast;
pub mod builtins;
pub mod error;
pub mod eval;
mod lexer;
pub mod parser;
pub mod stmt;
pub mod value;

pub use ast::{BinaryOp, Component, Expr, Parsed, UnaryOp, Var};
pub use error::ExprError;
pub use eval::{ElementScope, EvalCtx, GeoQueries, ParamRefs, SceneTime, eval};
pub use parser::{MAX_CALLS, MAX_DEPTH, MAX_SOURCE_LEN, parse};
pub use stmt::{MAX_STATEMENTS, Program, Runner, parse_program};
pub use value::Value;
