//! The expression syntax tree.
//!
//! Deliberately small. There is no string type in the value lattice
//! (decision M-3), so [`Expr::Str`] exists only as a call argument: `ch()`
//! and `bbox()` take a path or a field name, and the parser refuses a
//! string anywhere a value is expected. That keeps every evaluated value
//! inside the numeric union the resolver already knows how to conform.

use std::ops::Range;

/// A read-only variable, resolved from the evaluation context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    /// `$T`, scene seconds. Zero while the runtime is stopped.
    Time,
    /// `$F`, the current frame. Zero while the runtime is stopped.
    Frame,
    /// `$FPS`, frames per second.
    Fps,
    /// `$PI`.
    Pi,
    /// `$E`.
    E,
}

impl Var {
    /// The spelling, without the leading `$`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Var::Time => "T",
            Var::Frame => "F",
            Var::Fps => "FPS",
            Var::Pi => "PI",
            Var::E => "E",
        }
    }

    /// Whether reading this variable makes an expression time-dependent.
    ///
    /// This is what the runtime's tick keys on: a scene with no
    /// time-referencing expression pays nothing per frame.
    #[must_use]
    pub fn is_time(self) -> bool {
        matches!(self, Var::Time | Var::Frame)
    }

    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "T" => Some(Var::Time),
            "F" => Some(Var::Frame),
            "FPS" => Some(Var::Fps),
            "PI" => Some(Var::Pi),
            "E" => Some(Var::E),
            _ => None,
        }
    }
}

/// A vector component selected by `.x` / `.y` / `.z` / `.w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    X,
    Y,
    Z,
    W,
}

impl Component {
    /// The lane this component reads.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Component::X => 0,
            Component::Y => 1,
            Component::Z => 2,
            Component::W => 3,
        }
    }

    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "x" => Some(Component::X),
            "y" => Some(Component::Y),
            "z" => Some(Component::Z),
            "w" => Some(Component::W),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    /// A string literal. Legal only as a call argument.
    Str(String),
    Var(Var),
    Unary {
        op: UnaryOp,
        rhs: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    Member {
        base: Box<Expr>,
        component: Component,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        /// The call's span, so an arity or name error underlines the call
        /// rather than the whole expression.
        span: Range<usize>,
    },
}

impl Expr {
    /// Every literal path this tree passes to `ch()`.
    ///
    /// The dependency index is built from this: a path is a *string
    /// literal* by construction (there is no string type to compute one
    /// with, decision M-3), so the set of things an expression can read is
    /// fully known without evaluating it. That is what makes the index
    /// derivable from the document alone.
    #[must_use]
    pub fn ch_paths(&self) -> Vec<String> {
        self.ch_calls().into_iter().map(|(_, p)| p).collect()
    }

    /// Every `ch()` call as `(span of the whole call, path)`.
    ///
    /// The span is what makes renaming safe: rewriting a path by searching
    /// the source for its text would also hit an identical string passed to
    /// `bbox()`, or a substring of a longer path. Editing user text on the
    /// user's behalf has to be exact.
    #[must_use]
    pub fn ch_calls(&self) -> Vec<(Range<usize>, String)> {
        let mut out = Vec::new();
        self.collect_ch_paths(&mut out);
        out
    }

    fn collect_ch_paths(&self, out: &mut Vec<(Range<usize>, String)>) {
        match self {
            Expr::Number(_) | Expr::Str(_) | Expr::Var(_) => {}
            Expr::Unary { rhs, .. } => rhs.collect_ch_paths(out),
            Expr::Binary { lhs, rhs, .. } => {
                lhs.collect_ch_paths(out);
                rhs.collect_ch_paths(out);
            }
            Expr::Ternary {
                cond,
                then,
                otherwise,
            } => {
                // Both branches count, not just the taken one: the index
                // has to know everything this param COULD read, or a
                // condition flipping would leave a stale dependency.
                cond.collect_ch_paths(out);
                then.collect_ch_paths(out);
                otherwise.collect_ch_paths(out);
            }
            Expr::Member { base, .. } => base.collect_ch_paths(out),
            Expr::Call { name, args, span } => {
                if name == "ch"
                    && let [Expr::Str(path)] = args.as_slice()
                {
                    out.push((span.clone(), path.clone()));
                }
                for a in args {
                    a.collect_ch_paths(out);
                }
            }
        }
    }
}

/// A parsed expression plus the facts the engine caches about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub root: Expr,
    /// True when the tree reads `$T` or `$F`, so the runtime can dirty
    /// only the nodes a tick can actually change.
    pub uses_time: bool,
}
