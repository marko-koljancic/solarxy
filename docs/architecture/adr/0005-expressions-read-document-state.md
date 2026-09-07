# 0005. Parameter expressions read document state, so they need no edges in the cook order

- **Status**: accepted
- **Date**: 2026-09-07

## Context

A parameter can be an expression rather than a literal, and an expression can read another
node's parameter: `ch("../geo1/rotate")`, `ch("sphere1/radius")`, `ch("height")` on the node
itself. That is the mechanism by which one control node drives many parameters, and it is the
only mechanism by which a value crosses between networks, because nodes placeable in the
object network are portless by registry invariant and so cannot be wired at all.

The obvious implementation is to treat such a reference as an edge: add it to the graph the
cook is ordered by, and the referenced value is guaranteed to exist before the referring node
runs. That is what a reader expects, and it is wrong here, for a reason that is written into
the module that implements the alternative
(`crates/solarxy-graph/src/refs.rs`, module header): a reference reads a *parameter*, which is
document state, not a cook output, so it "needs no change to the wire topology and no virtual
edges, and cook order is irrelevant: a referenced expression is evaluated on demand, right
here, by recursing. The dependency graph exists only to know what to re-dirty and what to
refuse, never to order anything."

This is the clearest case in the codebase of a decision already articulated where it is
implemented.

## Options considered

### Option A: references read document state and resolve on demand

An expression is evaluated during the referring node's own parameter resolution; a `ch()` hop
recurses into the referenced parameter and evaluates it there and then. A separate index
exists purely to answer "who reads this" for invalidation and "would this close a loop" for
refusal.

### Option B: a reference is a virtual edge in the cook graph

Add each `ch()` target as an edge and let the topological sort guarantee the ordering.

It costs three things. Cook order would become sensitive to parameter references, so a
reference into another network would impose an ordering between networks that has no
geometric meaning. A loop among parameters would become a cycle in the cook graph, and the
cook graph is node-level, so `width = ch("height")` on a single node, which is legal and
useful, would be refused as a cycle. And the topology would have to be rebuilt on every
parameter edit rather than only on a wiring change.

### Option C: no cross-node parameter references

Everything is a wire. A value that drives two nodes comes from a node with two outputs.

It removes the whole subsystem and it removes the capability with it. Cross-context values
have no wire available by construction, so material and texture networks could not be driven
from the object network at all, and a shared numeric constant would cost a node and two wires
per consumer.

## Decision

A `ch()` reference reads document state. It is resolved on demand, by recursion, at the point
where the referring parameter is evaluated. It creates no edge in the cook order, and cook
order remains pure wire topology.

A dependency index over `(node, parameter key)` pairs exists for two jobs and no others: to
re-dirty everything that reads a parameter when it changes, and to refuse a reference that
would close a loop at the moment it is written.

## Consequences

The index is derivable without evaluating anything, and that is a property of the language
rather than of the index. A path is a string literal by construction: there is no string type
in the expression value lattice, so the set of things an expression can read is statically
known (`crates/solarxy-graph/src/expr/ast.rs`). Adding string concatenation to the language
would silently invalidate the entire index.

It is rebuilt from the document after any command that could change a reference, never
patched, and the file argues that trade with a measurement: the scan-based alternative it
replaced cost 1.68 ms per parameter write at 210 nodes and 25.5 ms at 840, while rebuilding is
one linear pass per user command against constant-time lookups during propagation, and it makes
a stale entry structurally impossible (`crates/solarxy-graph/src/refs.rs:328`).

Keys are pairs rather than nodes deliberately, so a node reading one of its own parameters is
legal.

Refusal happens at write time. Before a parameter is stored, the engine resolves what the
not-yet-stored expression would read and refuses if any target already reaches the referring
pair, returning a typed error and leaving the parameter untouched
(`crates/solarxy-graph/src/engine/mod.rs:1800`). The index collects paths from both branches of
a ternary, not only the taken one, because the index has to know everything a parameter could
read.

The backstop is a depth cap. `MAX_REF_DEPTH = 32` (`crates/solarxy-graph/src/refs.rs:32`) is
checked on each recursion, and its own documentation names what it is for: the paths that
bypass the write-time check, meaning a hand-edited document or a pasted fragment. Exceeding it
badges the node with an error rather than overflowing the stack.

The confirmed gap: a wrangle program's `ch()` reads create no edge. The index walks only
parameters whose source is an expression (`crates/solarxy-graph/src/refs.rs:412`), while a
wrangle's program is a snippet stored as a text literal that the cook nonetheless hands the
full reference capability, and the node's own shipped documentation advertises `ch()` inside
it. So editing a parameter does not re-dirty a wrangle that reads it, and renaming a node does
not rewrite the path inside the program, though the identical text in a parameter expression is
rewritten correctly. The gap bites only for a read that is not also wire-upstream; a wrangle
wired below the node it reads still recooks for the ordinary topological reason.

Enforcement: the write-time refusal and the depth cap are code. The index's completeness is
not checked by anything: no test asserts that a wrangle's `ch()` produces an edge, and the two
files that carry the most architectural weight here, `refs.rs` and `expr/ast.rs`, contain no
unit tests of their own.

## Notes

One semantic surprise, load-bearing and documented only in the code: `ch()` on a parameter
declared in degrees returns degrees, not the radians the cook body receives, because the unit
conversion is skipped specifically so that copying a rotation from one node to another
round-trips instead of landing 57 times off.
