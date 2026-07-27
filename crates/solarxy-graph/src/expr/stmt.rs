//! The wrangle's statement layer: a small program run once per element.
//!
//! This is the expression grammar plus three things it does not have:
//! statements separated by `;`, assignment, and an element scope
//! (`@attribute` reads and writes, plus typed locals). Everything else --
//! operators, precedence, the ~30 builtins, `ch()`, the geometry queries,
//! `$T` -- is the *same* code, reached through
//! [`super::parser::parse_scoped`]. Forking the grammar would be two
//! grammars to keep in step.
//!
//! **No control flow** (decision M-4). There is no `if` and no `for`, so a
//! program's cost is exactly (statements x elements) and cannot run away on
//! a single-threaded cook. A conditional value is still expressible with
//! the ternary the expression grammar already has.
//!
//! **Slots, not names.** `@P` and a local both resolve to an index while
//! parsing, so the per-element loop never hashes a string. The names
//! survive in [`Program::lanes`] and [`Program::locals`] for error
//! messages and for telling the kernel which lanes to bind.
//!
//! **Errors carry line and column** (decision M-22), via
//! [`super::error::ExprError::line_col`]. A parse failure is a cook error;
//! an arithmetic condition such as division by zero is *not*, because it
//! yields the IEEE result and one bad element must not blank a scene.

use std::collections::HashMap;

use super::ast::{Expr, Parsed};
use super::error::ExprError;
use super::eval::{ElementScope, EvalCtx, eval};
use super::lexer::{Tok, Token, lex};
use super::parser::{MAX_SOURCE_LEN, Scope, parse_tokens};
use super::value::Value;

/// Maximum statements in one program. The sandbox is a count rather than a
/// runtime budget for the same reason the expression limits are: with no
/// loops, static size bounds the work exactly.
pub const MAX_STATEMENTS: usize = 256;

/// A declared local's width, from its type keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalType {
    /// `float`
    Float,
    /// `vector2`
    Vec2,
    /// `vector`
    Vec3,
    /// `vector4`
    Vec4,
}

impl LocalType {
    fn from_keyword(word: &str) -> Option<Self> {
        match word {
            "float" => Some(LocalType::Float),
            "vector2" => Some(LocalType::Vec2),
            "vector" => Some(LocalType::Vec3),
            "vector4" => Some(LocalType::Vec4),
            _ => None,
        }
    }

    fn width(self) -> usize {
        match self {
            LocalType::Float => 1,
            LocalType::Vec2 => 2,
            LocalType::Vec3 => 3,
            LocalType::Vec4 => 4,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            LocalType::Float => "float",
            LocalType::Vec2 => "vector2",
            LocalType::Vec3 => "vector",
            LocalType::Vec4 => "vector4",
        }
    }
}

/// One lane the program touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    /// The *storage* name, which is what the kernel binds. Differs from
    /// [`Lane::spelling`] only where the language's conventional name and
    /// the reserved constant disagree: `@Cd` stores as `color`.
    pub name: String,
    /// The name as the user wrote it, so an error about `@Cd` says `@Cd`
    /// rather than naming a lane they never typed.
    pub spelling: String,
    /// The program assigns it somewhere.
    pub written: bool,
}

/// Maps a written attribute name to the lane it stores as.
///
/// `@Cd` is the one place the conventional spelling and the reserved
/// constant differ: every wrangle language calls per-point colour `Cd`, and
/// [`solarxy_kernel::reserved::COLOR`] calls it `color`. Mapping here means
/// a wrangle writing `@Cd` lights up the vertex-colour channel 0.8.0
/// shipped, with no extra step.
fn storage_name(spelling: &str) -> &str {
    match spelling {
        "Cd" => solarxy_kernel::reserved::COLOR,
        other => other,
    }
}

/// One declared local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub ty: LocalType,
}

/// Where a statement writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A lane slot in [`Program::lanes`].
    Attr(usize),
    /// A register in [`Program::locals`].
    Local(usize),
}

/// One statement: evaluate `value`, store it in `target`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub target: Target,
    pub value: Expr,
    /// The whole statement's span, so an error underlines the statement
    /// rather than the whole program.
    pub span: std::ops::Range<usize>,
}

/// A parsed wrangle program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub stmts: Vec<Stmt>,
    /// Every lane the program reads or writes, slot-indexed.
    pub lanes: Vec<Lane>,
    /// Every declared local, register-indexed.
    pub locals: Vec<Local>,
    /// True when any statement reads `$T` or `$F`.
    pub uses_time: bool,
}

impl Program {
    /// The lane bindings to hand [`solarxy_kernel::wrangle::wrangle`], in
    /// slot order so the kernel's slots and this program's slots agree.
    #[must_use]
    pub fn lane_bindings(&self) -> Vec<solarxy_kernel::wrangle::LaneBinding> {
        self.lanes
            .iter()
            .map(|l| solarxy_kernel::wrangle::LaneBinding {
                name: l.name.clone(),
                written: l.written,
            })
            .collect()
    }

    /// Whether the program writes anything at all. A program that only
    /// reads is legal but pointless, and the node says so rather than
    /// silently cooking its input through.
    #[must_use]
    pub fn writes_anything(&self) -> bool {
        self.lanes.iter().any(|l| l.written)
    }
}

/// The parse-time scope: interns lane and local names into slots.
struct WrangleScope {
    lanes: Vec<Lane>,
    lane_index: HashMap<String, usize>,
    locals: Vec<Local>,
    local_index: HashMap<String, usize>,
}

impl WrangleScope {
    fn new() -> Self {
        Self {
            lanes: Vec::new(),
            lane_index: HashMap::new(),
            locals: Vec::new(),
            local_index: HashMap::new(),
        }
    }

    /// Interns by STORAGE name, so `@Cd` and `@color` are one slot rather
    /// than two views of the same buffer that could disagree.
    fn intern_lane(&mut self, spelling: &str) -> usize {
        let storage = storage_name(spelling);
        if let Some(i) = self.lane_index.get(storage) {
            return *i;
        }
        let i = self.lanes.len();
        self.lanes.push(Lane {
            name: storage.to_string(),
            spelling: spelling.to_string(),
            written: false,
        });
        self.lane_index.insert(storage.to_string(), i);
        i
    }

    fn declare_local(&mut self, name: &str, ty: LocalType) -> usize {
        let i = self.locals.len();
        self.locals.push(Local {
            name: name.to_string(),
            ty,
        });
        // A redeclaration shadows: the name now resolves to the new slot.
        self.local_index.insert(name.to_string(), i);
        i
    }
}

impl Scope for WrangleScope {
    fn attr(&mut self, name: &str) -> Option<usize> {
        Some(self.intern_lane(name))
    }

    fn local(&mut self, name: &str) -> Option<usize> {
        self.local_index.get(name).copied()
    }
}

/// Parses a wrangle program.
///
/// # Errors
/// A malformed statement, an undeclared local, a program over the size
/// limits, or any error the expression grammar itself raises. Every error
/// carries a byte span; call [`ExprError::line_col`] for coordinates.
pub fn parse_program(source: &str) -> Result<Program, ExprError> {
    if source.len() > MAX_SOURCE_LEN {
        return Err(ExprError::new(
            format!(
                "the program is {} bytes; the limit is {MAX_SOURCE_LEN}",
                source.len()
            ),
            0..source.len().min(MAX_SOURCE_LEN),
        ));
    }
    let tokens = lex(source)?;
    let mut scope = WrangleScope::new();
    let mut stmts = Vec::new();
    let mut uses_time = false;

    for run in split_statements(&tokens, source.len())? {
        if stmts.len() >= MAX_STATEMENTS {
            return Err(ExprError::new(
                format!("the program has more than {MAX_STATEMENTS} statements"),
                run.span.clone(),
            ));
        }
        let stmt = parse_statement(run, source.len(), &mut scope)?;
        uses_time |= stmt.uses_time;
        stmts.push(stmt.stmt);
    }

    if stmts.is_empty() {
        return Err(ExprError::new(
            "the program is empty; assign something, for example `@Cd = set(1, 0, 0);`",
            0..source.len(),
        ));
    }

    Ok(Program {
        stmts,
        lanes: scope.lanes,
        locals: scope.locals,
        uses_time,
    })
}

/// One statement's tokens plus the span they cover.
struct Run {
    tokens: Vec<Token>,
    span: std::ops::Range<usize>,
}

/// Splits the token stream on `;`.
///
/// There are no blocks and no nested statements, so every `;` at any depth
/// is a statement boundary; a `;` inside parentheses is a syntax error the
/// expression parser reports, not a boundary this function has to track.
fn split_statements(tokens: &[Token], src_len: usize) -> Result<Vec<Run>, ExprError> {
    let mut runs = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    for token in tokens {
        if token.tok == Tok::Semi {
            if !current.is_empty() {
                runs.push(finish_run(std::mem::take(&mut current)));
            }
            continue;
        }
        current.push(token.clone());
    }
    // A trailing statement without its `;` is accepted: requiring the last
    // one would be a rule that exists only to be tripped over.
    if !current.is_empty() {
        runs.push(finish_run(current));
    }
    if runs.is_empty() {
        return Err(ExprError::new(
            "the program is empty; assign something, for example `@Cd = set(1, 0, 0);`",
            0..src_len,
        ));
    }
    Ok(runs)
}

fn finish_run(tokens: Vec<Token>) -> Run {
    let start = tokens.first().map_or(0, |t| t.span.start);
    let end = tokens.last().map_or(start, |t| t.span.end);
    Run {
        tokens,
        span: start..end,
    }
}

struct ParsedStmt {
    stmt: Stmt,
    uses_time: bool,
}

fn parse_statement(
    run: Run,
    src_len: usize,
    scope: &mut WrangleScope,
) -> Result<ParsedStmt, ExprError> {
    let span = run.span.clone();
    let mut tokens = run.tokens;

    // A leading type keyword declares a local. It is checked before the
    // assignment split so `float t = 1` reads as one thing.
    let declared = match tokens.first().map(|t| t.tok.clone()) {
        Some(Tok::Ident(word)) => LocalType::from_keyword(&word),
        _ => None,
    };

    let (target, rhs) = if let Some(ty) = declared {
        let keyword_span = tokens[0].span.clone();
        let Some(name_token) = tokens.get(1).cloned() else {
            return Err(ExprError::new(
                format!("`{}` needs a variable name after it", ty.describe()),
                keyword_span,
            ));
        };
        let Tok::Ident(name) = name_token.tok.clone() else {
            let what = name_token.tok.describe();
            return Err(ExprError::new(
                format!(
                    "expected a variable name after `{}`, found {what}",
                    ty.describe()
                ),
                name_token.span,
            ));
        };
        if LocalType::from_keyword(&name).is_some() {
            return Err(ExprError::new(
                format!("`{name}` is a type keyword and cannot be a variable name"),
                name_token.span,
            ));
        }
        match tokens.get(2).map(|t| t.tok.clone()) {
            Some(Tok::Assign) => {}
            Some(other) => {
                return Err(ExprError::new(
                    format!(
                        "expected `=` after `{} {name}`, found {}",
                        ty.describe(),
                        other.describe()
                    ),
                    tokens[2].span.clone(),
                ));
            }
            None => {
                return Err(ExprError::new(
                    format!(
                        "`{} {name}` needs a value; declaration without assignment is not \
                         supported",
                        ty.describe()
                    ),
                    span.clone(),
                ));
            }
        }
        let rhs = tokens.split_off(3);
        // Declared AFTER the right side is parsed below, so `float x = x;`
        // refers to an outer `x` rather than to itself. The slot is minted
        // here only because the target needs it; see the ordering note.
        (TargetSpec::Declare { name, ty }, rhs)
    } else {
        // An assignment splits at the first top-level `=`. `==` is a
        // distinct token, so a comparison is never mistaken for one.
        let Some(eq) = tokens.iter().position(|t| t.tok == Tok::Assign) else {
            let what = tokens
                .first()
                .map_or_else(|| "the statement".to_string(), |t| t.tok.describe());
            return Err(ExprError::new(
                format!(
                    "{what} is not a statement; every statement assigns, as in \
                     `@Cd = set(1, 0, 0)`"
                ),
                span.clone(),
            ));
        };
        let lhs: Vec<Token> = tokens[..eq].to_vec();
        let rhs = tokens[eq + 1..].to_vec();
        let target = parse_assign_target(&lhs, &span, scope)?;
        (target, rhs)
    };

    if rhs.is_empty() {
        return Err(ExprError::new("the assignment has no value", span.clone()));
    }

    let Parsed { root, uses_time } = parse_tokens(rhs, src_len, scope)?;

    // Declaring after parsing is what makes `float x = x` resolve to an
    // outer binding instead of to the half-built one.
    let target = match target {
        TargetSpec::Declare { name, ty } => Target::Local(scope.declare_local(&name, ty)),
        TargetSpec::Resolved(t) => t,
    };
    if let Target::Attr(slot) = target {
        scope.lanes[slot].written = true;
    }

    Ok(ParsedStmt {
        stmt: Stmt {
            target,
            value: root,
            span,
        },
        uses_time,
    })
}

enum TargetSpec {
    Declare { name: String, ty: LocalType },
    Resolved(Target),
}

fn parse_assign_target(
    lhs: &[Token],
    span: &std::ops::Range<usize>,
    scope: &mut WrangleScope,
) -> Result<TargetSpec, ExprError> {
    match lhs {
        [
            Token {
                tok: Tok::Attr(name),
                span: name_span,
            },
        ] => {
            if is_counter(name) {
                return Err(ExprError::new(
                    format!(
                        "`@{name}` is the element counter and is read-only; it describes \
                         where you are, not something you set"
                    ),
                    name_span.clone(),
                ));
            }
            Ok(TargetSpec::Resolved(Target::Attr(scope.intern_lane(name))))
        }
        [
            Token {
                tok: Tok::Ident(name),
                span: name_span,
            },
        ] => match scope.local_index.get(name) {
            Some(i) => Ok(TargetSpec::Resolved(Target::Local(*i))),
            None => Err(ExprError::new(
                format!(
                    "`{name}` is not declared; write `float {name} = ...` \
                     (or `vector {name} = ...`) the first time, or `@{name}` for an attribute"
                ),
                name_span.clone(),
            )),
        },
        [] => Err(ExprError::new(
            "the assignment has no target on its left",
            span.clone(),
        )),
        // Component assignment (`@P.x = 1`) is deliberately absent in v1;
        // it needs read-modify-write semantics the register file does not
        // model yet. Saying so beats "unexpected `.`".
        _ => Err(ExprError::new(
            "only a whole `@attribute` or a local can be assigned; \
             assign the whole value, as in `@P = set(@P.x * 2, @P.y, @P.z)`",
            lhs.first().map_or(span.clone(), |t| {
                t.span.start..lhs.last().map_or(t.span.end, |l| l.span.end)
            }),
        )),
    }
}

/// The four read-only counters the element scope answers from the loop
/// rather than from a buffer.
fn is_counter(name: &str) -> bool {
    matches!(name, "ptnum" | "numpt" | "primnum" | "numprim")
}

/// The per-element register file, reused across elements.
///
/// This is the bridge between the kernel's untyped slots and the
/// expression layer's [`Value`]: the kernel owns the buffers, this owns the
/// interpretation.
pub struct Registers<'a> {
    slots: &'a [solarxy_kernel::wrangle::Slot],
    locals: &'a [Option<Value>],
    lanes: &'a [Lane],
    local_decls: &'a [Local],
    index: usize,
    count: usize,
}

impl ElementScope for Registers<'_> {
    fn attr(&self, slot: usize) -> Result<Value, String> {
        // The two counters are lanes by name so the grammar needs no
        // special case, but they are answered here rather than read from a
        // buffer that does not exist.
        match self.lanes.get(slot).map(|l| l.spelling.as_str()) {
            Some("ptnum" | "primnum") => return Ok(Value::Float(self.index as f64)),
            Some("numpt" | "numprim") => return Ok(Value::Float(self.count as f64)),
            _ => {}
        }
        let Some(s) = self.slots.get(slot) else {
            return Err("attribute slot out of range".to_string());
        };
        if !s.present {
            let name = self.lanes.get(slot).map_or("?", |l| l.spelling.as_str());
            return Err(format!(
                "`@{name}` is not on the incoming geometry, so it has no value to read; \
                 assign it before reading it"
            ));
        }
        value_from_slot(s).ok_or_else(|| {
            let name = self.lanes.get(slot).map_or("?", |l| l.spelling.as_str());
            format!("`@{name}` has an unsupported width")
        })
    }

    fn local(&self, slot: usize) -> Result<Value, String> {
        if let Some(v) = self.locals.get(slot).and_then(|v| *v) {
            return Ok(v);
        }
        let name = self.local_decls.get(slot).map_or("?", |l| l.name.as_str());
        Err(format!("`{name}` is read before it is assigned"))
    }
}

fn value_from_slot(slot: &solarxy_kernel::wrangle::Slot) -> Option<Value> {
    match slot.width {
        1 => Some(Value::Float(slot.value[0])),
        2 => Some(Value::Vec2([slot.value[0], slot.value[1]])),
        3 => Some(Value::Vec3([slot.value[0], slot.value[1], slot.value[2]])),
        4 => Some(Value::Vec4(slot.value)),
        _ => None,
    }
}

/// A [`Program`] bound to an evaluation context, ready to run per element.
///
/// Holds the locals' register file, allocated once and reused, which is
/// what keeps the inner loop free of allocation.
pub struct Runner<'a> {
    program: &'a Program,
    /// The context minus its element scope; the scope is per element.
    base: EvalCtx<'a>,
    source: &'a str,
}

impl<'a> Runner<'a> {
    #[must_use]
    pub fn new(program: &'a Program, base: EvalCtx<'a>, source: &'a str) -> Self {
        Self {
            program,
            base,
            source,
        }
    }

    /// The message an error carries, located to line and column.
    fn locate(&self, e: &ExprError) -> String {
        let (line, col) = e.line_col(self.source);
        format!("line {line}, column {col}: {}", e.message)
    }
}

impl solarxy_kernel::wrangle::ElementFn for Runner<'_> {
    fn run(&self, element: &mut solarxy_kernel::wrangle::Element) -> Result<(), String> {
        let mut locals: Vec<Option<Value>> = vec![None; self.program.locals.len()];

        for stmt in &self.program.stmts {
            let value = {
                let registers = Registers {
                    slots: element.slots,
                    locals: &locals,
                    lanes: &self.program.lanes,
                    local_decls: &self.program.locals,
                    index: element.index,
                    count: element.count,
                };
                let ctx = EvalCtx {
                    element: Some(&registers),
                    ..self.base
                };
                eval(&stmt.value, &ctx).map_err(|e| self.locate(&e))?
            };

            match stmt.target {
                Target::Local(slot) => {
                    let ty = self.program.locals[slot].ty;
                    let widened = widen(value, ty.width()).ok_or_else(|| {
                        format!(
                            "`{}` is a {} and cannot hold a {}",
                            self.program.locals[slot].name,
                            ty.describe(),
                            value.type_name()
                        )
                    })?;
                    locals[slot] = Some(widened);
                }
                Target::Attr(slot) => {
                    let lanes = value.lanes().ok_or_else(|| {
                        format!(
                            "`@{}` cannot be assigned a {}",
                            self.program.lanes[slot].spelling,
                            value.type_name()
                        )
                    })?;
                    let out = &mut element.slots[slot];
                    out.value = [0.0; 4];
                    out.value[..lanes.len()].copy_from_slice(&lanes);
                    out.width = u8::try_from(lanes.len()).unwrap_or(0);
                    out.present = true;
                    out.assigned = true;
                }
            }
        }
        Ok(())
    }
}

/// Fits a value to a declared width, broadcasting a scalar the way every
/// binary operator already does. A wider value into a narrower local is an
/// error rather than a truncation, for the reason `map2` gives: silently
/// dropping `z` is a bug nobody finds.
fn widen(value: Value, width: usize) -> Option<Value> {
    let lanes = value.lanes()?;
    if lanes.len() == width {
        return Some(value);
    }
    if lanes.len() == 1 {
        return Value::from_lanes(&vec![lanes[0]; width]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::SceneTime;
    use solarxy_kernel::AttributeDomain;
    use solarxy_kernel::set::{AttributeData, GeometrySet};
    use solarxy_kernel::wrangle::wrangle;
    use std::sync::Arc;

    fn program(src: &str) -> Program {
        parse_program(src).unwrap_or_else(|e| panic!("parse failed: {} @ {:?}", e.message, e.span))
    }

    fn err(src: &str) -> ExprError {
        parse_program(src).expect_err("expected a parse error")
    }

    /// Runs `src` over a unit box and returns the result.
    fn run(src: &str, domain: AttributeDomain) -> Result<GeometrySet, String> {
        let set = GeometrySet::from_parts(
            vec![solarxy_kernel::primitives::generate_box(
                1.0, 1.0, 1.0, 1, 1, 1,
            )],
            Vec::new(),
        );
        run_on(&set, src, domain)
    }

    fn run_on(
        set: &GeometrySet,
        src: &str,
        domain: AttributeDomain,
    ) -> Result<GeometrySet, String> {
        let prog = program(src);
        let base = EvalCtx::new(SceneTime::default());
        let runner = Runner::new(&prog, base, src);
        wrangle(set, domain, &prog.lane_bindings(), &runner).map_err(|e| e.to_string())
    }

    fn float_lane<'a>(set: &'a GeometrySet, name: &str) -> &'a [f32] {
        match set.meshes[0].attributes.get(name) {
            Some(AttributeData::Float(v)) => v.as_slice(),
            other => panic!("expected a Float lane `{name}`, found {other:?}"),
        }
    }

    // ---- parsing ----

    #[test]
    fn a_single_attribute_assignment_parses_into_one_statement() {
        let p = program("@Cd = set(1, 0, 0);");
        assert_eq!(p.stmts.len(), 1);
        assert_eq!(p.lanes.len(), 1);
        // Stored under the reserved constant, but still spelled as written.
        assert_eq!(p.lanes[0].name, solarxy_kernel::reserved::COLOR);
        assert_eq!(p.lanes[0].spelling, "Cd");
        assert!(p.lanes[0].written);
        assert!(p.writes_anything());
    }

    #[test]
    fn a_trailing_semicolon_is_optional() {
        assert_eq!(program("@Cd = 1").stmts.len(), 1);
        assert_eq!(program("@Cd = 1;").stmts.len(), 1);
    }

    #[test]
    fn a_lane_read_and_written_interns_to_one_slot() {
        let p = program("@Cd = @Cd * 2;");
        assert_eq!(p.lanes.len(), 1, "one slot, not two");
        assert!(p.lanes[0].written);
    }

    #[test]
    fn a_read_only_lane_is_not_marked_written() {
        let p = program("@out = @in * 2;");
        let inp = p.lanes.iter().find(|l| l.name == "in").expect("in");
        let out = p.lanes.iter().find(|l| l.name == "out").expect("out");
        assert!(!inp.written);
        assert!(out.written);
    }

    #[test]
    fn typed_locals_declare_their_width() {
        let p = program("float t = 1; vector v = set(1,2,3); vector4 c = set(1,2,3,4); @Cd = c;");
        assert_eq!(p.locals.len(), 3);
        assert_eq!(p.locals[0].ty, LocalType::Float);
        assert_eq!(p.locals[1].ty, LocalType::Vec3);
        assert_eq!(p.locals[2].ty, LocalType::Vec4);
    }

    #[test]
    fn an_undeclared_local_names_itself_rather_than_reading_as_zero() {
        let e = err("t = 1;");
        assert!(e.message.contains("`t` is not declared"), "{}", e.message);
    }

    #[test]
    fn a_statement_that_does_not_assign_says_so() {
        let e = err("@P * 2;");
        assert!(
            e.message.contains("every statement assigns"),
            "{}",
            e.message
        );
    }

    #[test]
    fn component_assignment_explains_itself_instead_of_saying_unexpected_dot() {
        let e = err("@P.x = 1;");
        assert!(
            e.message.contains("assign the whole value"),
            "{}",
            e.message
        );
    }

    #[test]
    fn a_counter_is_read_only() {
        let e = err("@ptnum = 3;");
        assert!(e.message.contains("read-only"), "{}", e.message);
    }

    #[test]
    fn an_empty_program_suggests_what_to_write() {
        let e = err("   ");
        assert!(e.message.contains("assign something"), "{}", e.message);
    }

    #[test]
    fn a_comparison_is_not_mistaken_for_an_assignment() {
        // `==` is its own token, so the split on `=` must not see it.
        let p = program("@hit = @P.x == 0.5;");
        assert_eq!(p.stmts.len(), 1);
    }

    #[test]
    fn uses_time_is_true_only_when_a_statement_reads_the_clock() {
        assert!(!program("@Cd = 1;").uses_time);
        assert!(program("@P = @P * sin($T);").uses_time);
        assert!(program("@P = @P * $F;").uses_time);
        // $FPS is a setting, not a clock reading: it does not move per tick.
        assert!(!program("@Cd = $FPS;").uses_time);
    }

    #[test]
    fn the_statement_limit_is_enforced() {
        let src = "@a = 1;".repeat(MAX_STATEMENTS + 1);
        let e = parse_program(&src).expect_err("over the limit");
        assert!(e.message.contains("more than"), "{}", e.message);
    }

    #[test]
    fn a_parse_error_carries_a_line_and_column() {
        let src = "@Cd = 1;\n@P = ;";
        let e = parse_program(src).expect_err("bad statement");
        let (line, _col) = e.line_col(src);
        assert_eq!(line, 2, "the error points at the second line");
    }

    // ---- evaluation ----

    #[test]
    fn a_program_computes_per_element_from_position() {
        let out = run("@height = @P.y;", AttributeDomain::Point).expect("runs");
        let heights = float_lane(&out, "height");
        let set = GeometrySet::from_parts(
            vec![solarxy_kernel::primitives::generate_box(
                1.0, 1.0, 1.0, 1, 1, 1,
            )],
            Vec::new(),
        );
        for (h, p) in heights.iter().zip(set.meshes[0].positions.iter()) {
            assert!((h - p[1]).abs() < 1e-6, "{h} vs {}", p[1]);
        }
    }

    #[test]
    fn locals_carry_between_statements() {
        let out = run("float t = @P.x * 3; @v = t + 1;", AttributeDomain::Point).expect("runs");
        let v = float_lane(&out, "v");
        assert!(!v.is_empty());
    }

    #[test]
    fn a_scalar_broadcasts_into_a_wider_local() {
        let out = run("vector v = 0; @heat3 = v;", AttributeDomain::Point).expect("runs");
        assert!(matches!(
            out.meshes[0].attributes.get("heat3"),
            Some(AttributeData::Vec3(_))
        ));
    }

    #[test]
    fn a_wider_value_into_a_narrower_local_is_an_error_not_a_truncation() {
        let e = run("float t = set(1,2,3); @v = t;", AttributeDomain::Point).unwrap_err();
        assert!(e.contains("cannot hold"), "{e}");
    }

    #[test]
    fn reading_a_lane_the_input_lacks_names_the_lane() {
        let e = run("@out = @missing * 2;", AttributeDomain::Point).unwrap_err();
        assert!(e.contains("@missing"), "{e}");
        assert!(e.contains("not on the incoming geometry"), "{e}");
    }

    #[test]
    fn a_lane_assigned_earlier_is_readable_later_in_the_same_program() {
        let out = run("@heat = 2; @twice = @heat * 2;", AttributeDomain::Point).expect("runs");
        for v in float_lane(&out, "twice") {
            assert!((v - 4.0).abs() < 1e-6, "{v}");
        }
    }

    #[test]
    fn ptnum_and_numpt_are_answered_without_a_buffer() {
        let out = run("@i = @ptnum; @n = @numpt;", AttributeDomain::Point).expect("runs");
        let i = float_lane(&out, "i");
        let n = float_lane(&out, "n");
        let count = out.meshes[0].positions.len();
        assert!(
            i.iter()
                .enumerate()
                .all(|(k, v)| (*v - k as f32).abs() < 1e-6)
        );
        assert!(n.iter().all(|v| (*v - count as f32).abs() < 1e-6));
    }

    #[test]
    fn writing_cd_produces_the_reserved_colour_lane_the_renderer_reads() {
        let out = run("@Cd = set(1, 0, 0);", AttributeDomain::Point).expect("runs");
        let lane = out.meshes[0]
            .attributes
            .get(solarxy_kernel::reserved::COLOR)
            .expect("the reserved colour lane");
        // Widened to RGBA: the lane is contractually Vec4, and every
        // wrangle language writes colour as three components.
        let AttributeData::Vec4(rgba) = lane else {
            panic!("expected the colour lane to be Vec4, found {lane:?}");
        };
        assert!(
            rgba.iter().all(|c| (c[3] - 1.0).abs() < 1e-6),
            "opaque alpha"
        );
        assert!(rgba.iter().all(|c| (c[0] - 1.0).abs() < 1e-6), "red");
    }

    #[test]
    fn division_by_zero_is_an_ieee_value_not_a_cook_failure() {
        // Decision M-22: one bad element must not blank a scene.
        let out = run("@v = 1 / 0;", AttributeDomain::Point).expect("must not fail");
        assert!(float_lane(&out, "v").iter().all(|v| v.is_infinite()));
    }

    #[test]
    fn time_reads_zero_while_the_runtime_is_stopped() {
        let out = run("@t = $T;", AttributeDomain::Point).expect("runs");
        assert!(float_lane(&out, "t").iter().all(|v| *v == 0.0));
    }

    #[test]
    fn assigning_p_moves_the_geometry() {
        let out = run("@P = set(@P.x, @P.y + 1, @P.z);", AttributeDomain::Point).expect("runs");
        let before = GeometrySet::from_parts(
            vec![solarxy_kernel::primitives::generate_box(
                1.0, 1.0, 1.0, 1, 1, 1,
            )],
            Vec::new(),
        );
        for (a, b) in out.meshes[0]
            .positions
            .iter()
            .zip(before.meshes[0].positions.iter())
        {
            assert!((a[1] - (b[1] + 1.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn the_primitive_domain_writes_a_primitive_lane() {
        let out = run("@pid = @primnum;", AttributeDomain::Primitive).expect("runs");
        assert!(!out.meshes[0].attributes.contains_key("pid"));
        let lane = out.meshes[0]
            .primitive_attributes
            .get("pid")
            .expect("primitive lane");
        assert_eq!(lane.len(), out.meshes[0].primitive_count());
    }

    #[test]
    fn an_existing_lane_is_read_and_rewritten_in_place() {
        let mut set = GeometrySet::from_parts(
            vec![solarxy_kernel::primitives::generate_box(
                1.0, 1.0, 1.0, 1, 1, 1,
            )],
            Vec::new(),
        );
        let count = set.meshes[0].positions.len();
        set.meshes[0].attributes.insert(
            "heat".to_string(),
            AttributeData::Float(Arc::new(vec![2.0; count])),
        );
        let out = run_on(&set, "@heat = @heat * 5;", AttributeDomain::Point).expect("runs");
        assert!(
            float_lane(&out, "heat")
                .iter()
                .all(|v| (*v - 10.0).abs() < 1e-6)
        );
    }
}
