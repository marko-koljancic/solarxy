# 0006. A scene is one self-contained archive with content-addressed assets and a reader gate

- **Status**: accepted
- **Date**: 2026-09-07
- **Embodied in**: 0.7.0, the release at which the schema version was frozen at 1 and the
  compatibility promise below begins. The container predates it; version 0 was the pre-beta
  format and carried no guarantees. Version 2 arrived at 0.10.0 with the context vocabulary,
  and is the first bump the promise actually had to carry.

## Context

A scene references models, textures and environment maps. If the file records paths to them,
it stops working the moment it is moved, sent to someone else, or opened in a browser, where
there is no path to record. The browser is a primary surface here, so path references were
never an option for the shared format.

A scene also has to survive its own product. Node types gain parameters, the document gains
sections, and files written by an older or a newer build have to do something predictable
rather than something arbitrary.

And the writer runs inside WebAssembly, which rules out anything needing a C compression
backend.

## Options considered

### Option A: a ZIP container with a manifest, a scene document, and content-addressed blobs

One file: `manifest.json`, `scene.json`, and one `assets/<sha256>` entry per distinct asset.
Every entry Stored rather than deflated. Two version integers in the scene document, one
saying what shape it is and one saying what a reader must implement.

### Option B: one JSON file with assets inlined as base64

Simplest possible container: there is no container. Everything is one document.

Base64 inflates binary payload by a third and forces every byte of every model through the
JSON parser. A scene carrying a 200 MB model becomes a single string that has to be parsed
before anything can be inspected, and deduplicating two references to the same texture means
comparing two base64 strings.

### Option C: a project directory rather than a file

A folder with the document and the assets beside it, as several digital-content tools do.

It gives up the property this format exists for. A scene stops being one thing that can be
sent, downloaded, uploaded, stored in a browser's origin-private filesystem, or attached to a
support report. The browser has no directory to hand a user, so the two shells would need
different formats.

## Decision

A scene is one ZIP archive containing exactly `manifest.json`, `scene.json`, and one
`assets/<sha256>` entry per distinct asset. Every entry is Stored, uncompressed. Assets are
content-addressed by the lowercase hex SHA-256 of their bytes, which is simultaneously the
identifier in the document, the manifest entry, the archive path, and the integrity check.

The scene document carries two integers: `schema_version`, the shape it was written in, and
`min_reader`, the lowest reader version able to open it.

## Consequences

The compression choice is stated where it is made
(`crates/solarxy-scenefile/src/archive.rs`): Stored keeps the path pure Rust so it compiles to
`wasm32` without a C compression backend, and asset blobs are usually already compressed while
the scene document is small. Deflate is a later size optimisation, not a correctness question.

Content addressing does more than name blobs. Identical bytes are stored once, and because
that collapses byte-identical companion files into one entry, the record carries the alias
names so a by-name resolver still finds every companion after a load. Integrity is recomputed
on read for every manifest entry, and a hash or size mismatch fails the whole load before the
open document is touched, so a corrupt file cannot damage the session.

The compatibility promise, precisely. This build opens any file whose `min_reader` is at most
1, whose `schema_version` field is present, and whose assets all hash correctly. It refuses
exactly three things: a `min_reader` above what it implements, an absent `schema_version`, and
an integrity or size failure. A `schema_version` from the future loads best-effort with a
warning, leaning on defaulted sections. Unknown top-level keys warn and load, and the format
never uses strict field checking, so an unknown field inside a known section is ignored.

Four limits follow, and they are limits rather than nuances. The two version fields are guarded
asymmetrically: an absent `schema_version` is fatal, with a comment explaining that defaulting
it made a corrupt file indistinguishable from an old one, while an absent `min_reader` silently
reads as 0 and passes the gate. The stricter gate is the one that can be omitted.

The container migration was a placeholder called once rather than in a loop, with one real step
from 0 to 1 that only restamped the version, while the function's own documentation said it
stepped a value up one version at a time. This paragraph named the failure precisely: the day
the schema version became 2, a version-0 file would run the 0 step, be restamped to 1, and be
read at the newer shape with no warning. That day was 0.10.0, and the loop landed first, ahead
of the step that would have exposed it. The driver stamps each version rather than the steps
doing it, so a step that forgets cannot leave a document that migrates again on every open.

The node-level machinery, which runs a per-type hook once per version step over raw JSON before
typing, was already a genuine loop and is unchanged.

The 1-to-2 step is also the first one that rewrites fields, and it establishes two rules the
next one should follow. It runs on raw JSON before typing, because an unmigrated node matches no
descriptor and the recovery path for an unknown type discards every parameter. And it writes an
explicit name onto any container that had none, because a node with no name answers to its
type's display name and expressions address nodes by name, so a display-name change silently
redirects a path. Neither rule is obvious from the outside and both are cheap to omit.

Parameters are not self-describing: a parameter's JSON is a bare number or a bare string, and
reading it back requires the registry's declared type to tell an integer from a float from an
enum key from an asset digest. So a scene cannot be fully interpreted by anything but a build
whose registry matches, and the checked-in JSON Schema types the parameter map as a free object
and can never validate it. That is the direct cost of
[0004](0004-node-type-descriptor-is-the-single-source.md), and it is what lets a migration hook
see raw values before they are typed.

Re-saving is lossy in one case. A node whose type is unknown or whose stored version is newer
than this build's loads as a non-cooking placeholder with its parameters dropped, while its
edges and port order survive, so the graph still looks intact. Two doc comments claim the
parameters are preserved verbatim; they are not.

Enforcement: the reader gate, the integrity check and the node migration loop are code. The
schema is pinned to the Rust types by a drift test. Nothing enforces that a node version bump
carries a migration, and there is no committed fixture at any old version anywhere in the
repository, so every migration test synthesises its input with today's writer and can only
exercise differences its author remembered to reproduce.

## Notes

One thing rides inside the format without being described by it. The per-pane display settings
live in a field the format declares opaque and round-trips uninterpreted, while both shells in
fact deserialise it into a specific unversioned Rust struct; the browser's own comment says
this persists a pane's look "with no scene-schema change and no reader-version gate". Adding a
non-defaulted field to that struct would silently revert every saved scene's pane display on
both shells. Separately, the desktop shell can read a scene but cannot write one.
