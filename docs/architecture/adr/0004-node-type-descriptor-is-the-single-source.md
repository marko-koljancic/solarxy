# 0004. One node type descriptor drives the user interface, the cook, the persisted form and its migrations

- **Status**: accepted
- **Date**: 2026-09-07

## Context

A node type has to be described to four audiences that do not talk to each other: the user
interface that draws its ports and its parameter widgets, the cook that evaluates it, the
file format that stores an instance of it, and the reader that opens a file written by an
older build.

Described four times, they drift, and the drift is silent in the worst direction. A parameter
added to the cook but not to the panel is unreachable. A parameter renamed in the panel but
not in the reader loads as its default. A port whose declared type disagrees with what the
cook produces is a connection the editor allows and the evaluation refuses.

The registry is 77 node types (`crates/solarxy-graph/src/nodes/mod.rs:238`), so four
descriptions would be 308 things to keep in step.

## Options considered

### Option A: one static in-tree descriptor per type

Each node file exports a function returning a `NodeTypeDescriptor` struct literal; a
hand-written vector collects them (`crates/solarxy-graph/src/nodes/mod.rs:92`); the registry
keys them by `type_id` and validates them at construction
(`crates/solarxy-graph/src/registry/mod.rs:371`).

### Option B: a data manifest

Node types declared in a checked-in data file, loaded at startup and matched to cook functions
by name.

The cook is Rust code and cannot live in the manifest, so this splits the declaration from the
behaviour it describes and adds a way for the two to disagree: a manifest entry with no cook,
a cook with no entry, a port type the manifest promises and the function does not honour. It
buys the ability to change a node type without recompiling, which is not a capability this
product wants, because a node type change is also a migration and a schema question.

### Option C: a trait implemented per node type, with a derive macro

Each node type is a type implementing a `Node` trait, with the declaration produced by a
derive.

A descriptor is data, and this wraps data in a compile-time layer that adds no capability.
The registry would still have to be a homogeneous collection to be enumerated, so the trait
objects would be flattened back into descriptors to produce the snapshot the frontend reads.
It also makes the declaration harder to read than a struct literal, which matters because the
declaration is the documentation.

## Decision

`NodeTypeDescriptor` (`crates/solarxy-graph/src/registry/mod.rs:291`) is the single
declaration of a node type. It carries the identity and version (`type_id`, `version`), the
presentation (`display_name`, `category`, `doc`, `search_aliases`, `glyph`, `role`), the
placement rules (`contexts`, and `opens` for a container), the wiring contract
(`inputs`, `outputs`, each a `PortSpec` with a data type, arity and default-port flag), the
declarative parameter schemas (`params`, each a `ParamSpec` with type, default, ranges, unit
and visibility condition), the bypass behaviour, the cook function pointer, and the optional
migration hook.

Everything else derives from it. The palette, the typed handles and the parameter panel read a
snapshot of it. The cook driver resolves parameters and gathers inputs from it. The scene file
reads a stored parameter map back under it. The loader runs its migration hook stepwise and
then drops any stored key the current descriptor does not declare.

## Consequences

A registry that constructs is a valid one: `Registry::with_descriptors` runs
`invariant_violations` (`crates/solarxy-graph/src/registry/mod.rs:413`), which checks the
identifier shape, at most one variadic input, single-arity outputs, one default port per
direction, unique port and parameter keys, that a bypass target exists, that defaults conform
to their declared type, that enum variants are non-empty, that a soft range sits inside its
hard range, that every visibility condition names an existing parameter other than itself,
that versions start at 1, and that anything placeable in the object network is portless.

The declaration is publishable. `RegistrySnapshot` is captured into `schemas/registry.json` by
an example, and `crates/solarxy-graph/tests/registry_drift.rs:40` compares the checked-in file
against the live registry as text, so a descriptor change that is not regenerated fails the
build. The generated node reference under `schemas/` is guarded the same way.

The honest limit: the engine still special-cases specific type identifiers in several places,
so the descriptor is the single source for geometry operators and not yet for everything.
Scene lowering dispatches on the string literals `"geo"`, `"camera"` and `"environment"` and
then on a six-arm `is_light` match (`crates/solarxy-graph/src/engine/scene.rs:92` and `:132`).
`transform_params_for` (`crates/solarxy-graph/src/engine/mod.rs:776`) is a hardcoded table
saying which transform roles a type has, even though `solarxy_core::gizmo::TransformParams`
exists so a node can declare them. `invoke_action` matches on type-identifier and key pairs.
Several more sites hardcode `"geo"`, `"transform"`, `"render"` and `"environment"`. So adding a
light, a camera or a manipulable node touches files outside `nodes/`, and the claim that
adding a node is two touch points holds for a plain geometry operator only.

That is a defect against this decision rather than a qualification of it, and the obvious fix
is not the obvious one: `NodeRole::Light` cannot simply replace `is_light`, because the
environment node also declares `role: NodeRole::Light`
(`crates/solarxy-graph/src/nodes/environment_node.rs`) while being lowered as an environment.
Whatever replaces the string matches has to distinguish those two.

A smaller instance of the same shape: the keep-last-good check and the cook statistics key on
the literal port name `"geometry"` rather than on the descriptor's declared default output,
which holds only because a registry test pins the naming convention.

Enforcement: the registry invariants and the two drift tests, for the descriptor itself.
Nothing prevents a new string match on a type identifier.

## Notes

Versioning rides the same declaration. A node instance stores its `type_version`, and the
loader replays the descriptor's migration hook once per version step over raw JSON before the
values are typed. Nothing correlates a version bump with the presence of a hook, which is
[0006](0006-slxy-scene-file-format.md)'s problem rather than this one's.
