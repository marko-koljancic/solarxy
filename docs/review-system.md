# Solarxy Review System — Anchor Stability Contract

The Review System lets reviewers place spatially-anchored text
annotations on a 3D model and save them to a `<model>.solarxy-review.json`
sidecar that travels with the asset. This document is the load-bearing
reference for **how anchors survive re-exports** — the question the
Review System rests on.

The short version: anchors are stable across re-exports that preserve
mesh order and face indexing, and degrade gracefully (with a clearly
marked stale state) across re-exports that don't.

If you're an artist or client exchanging review files, this doc tells
you what to expect when the model changes between rounds. If you're a
Solarxy maintainer contemplating future format changes, this doc tells
you what the wire-format guarantee is.

---

## The problem

A reviewer opens `hero_v3.glb`, places an annotation on the pauldron
("soften this edge by 30%"), saves `hero_v3.solarxy-review.json`, and
sends it back. The artist receives both files, re-exports the asset as
`hero_v4.glb` after the edit, opens the pair, and expects to see the
annotation **on the same triangle** so they can mark it Resolved.

Without a stability contract this is impossible: every exporter
produces slightly different mesh / face indices, the world-space
coordinates of the affected vertex move around, and annotations
"detach" from the geometry they were meant to comment on.

Solarxy's contract is designed for the realistic case — re-exports
that preserve topology (the common output of "fix material, re-export"
or "tweak a shader, re-export" workflows) — and provides a graceful
fallback for the harder case (full re-topology).

---

## The anchor structure

An anchor is the tuple `(mesh_index, face_index, barycentric)` with a
world-space fallback. The Rust type lives in
[`solarxy_core::review::AnchorPosition`](../crates/solarxy-core/src/review.rs):

```rust
pub struct AnchorPosition {
    /// Primary anchor: index into RawModelData.meshes.
    pub mesh_index: u32,

    /// Index into the mesh's index buffer (0 = first triangle, etc.).
    pub face_index: u32,

    /// Barycentric weights (u, v, w) on that triangle. Sum to 1.0.
    /// Used to position the marker inside the triangle, not just on a
    /// vertex.
    pub barycentric: [f32; 3],

    /// World-space position at creation time. Used to find the nearest
    /// face when (mesh_index, face_index) is no longer valid.
    pub world_pos_fallback: [f32; 3],
}
```

The on-disk JSON shape matches the Rust types one-for-one. See
[`solarxy-core/src/review.rs`](../crates/solarxy-core/src/review.rs)
for the canonical reference; the wire format is versioned via
`ReviewFile::format_version: u32` (currently `1`).

---

## What's stable across re-exports (the contract)

A re-export is **anchor-safe** if all three of the following hold:

| Invariant | Why it matters | Common violators |
|---|---|---|
| **Mesh order is preserved.** The Nth mesh in `RawModelData.meshes` corresponds to the same logical part across exports. | `mesh_index` is the primary discriminator. If mesh 3 becomes mesh 5, every annotation on mesh 3 lands on the wrong part. | Some Blender export options reorder meshes alphabetically; some Maya plugins reorder by material. |
| **Face indexing is preserved.** Triangle indices into a given mesh's index buffer are the same across exports. | `face_index` directly identifies the triangle. Even a single triangle insertion shifts every subsequent index. | Any "optimize meshes" / "weld vertices" / "smooth normals" pass that re-emits the index buffer. |
| **Vertex order within a face is preserved.** Triangle ABC stays as (A, B, C), not (B, A, C). | `barycentric` is relative to the vertex order in the index buffer. If the order flips, the marker lands on the mirror position inside the triangle. | "Recompute normals" passes that re-wind triangles. |

**Practical guidance for exporters and pipelines:**
- glTF / GLB exports from Blender preserve all three when you avoid the
  "Optimize for size" option and don't run a "Clean Up → Decimate Geometry"
  pass between exports.
- Maya's glTF exporter (via Maya2glTF) preserves all three by default;
  enabling "Merge by material" can break mesh order.
- Re-exporting with the same exporter version on the same source file
  is almost always anchor-safe. Re-exporting after a re-import + re-save
  in a different DCC tool usually is not.

---

## What breaks the contract

When the artist runs a re-topology pass (e.g. "remesh", "decimate", or
"rebuild from sculpt"), the mesh's index buffer is rebuilt from scratch.
Mesh indices, face indices, and triangle vertex orders all change.
Anchors lose their primary key.

The Review System detects this in two ways:

1. **Hash check.** Each entry in `ReviewFile.mesh_hashes` is a stable
   hash of the corresponding mesh's geometry at the time the review was
   created. On reload, Solarxy re-hashes the loaded mesh and compares.
   A mismatch marks every annotation on that mesh as **stale**.
2. **Anchor validity check.** Even when the hash matches, if
   `(mesh_index, face_index)` lies out of bounds (e.g. a smaller
   re-export with fewer triangles), the annotation is marked stale.

A stale annotation is **not deleted**. It moves from the "Open" section
of the Review Panel to a dedicated **Needs re-anchor** section, and its
marker in the 3D view renders dimmed at 50% alpha so it doesn't disappear
visually.

---

## Fallback behavior: `world_pos_fallback`

For stale annotations, the marker is placed at `world_pos_fallback` —
the world-space position computed when the annotation was first saved.
This is a deliberate *visual* fallback, not an automatic re-anchor:

- The marker shows up in roughly the right place so reviewers can see
  what was intended.
- The "Needs re-anchor" badge signals that the anchor is approximate
  and shouldn't be trusted for fine-grained spatial precision.
- The reviewer can **re-place** the marker on the new geometry via the
  Review Panel's `Re-place` button, which enters re-anchor sub-mode
  (single-click on the new triangle to commit). The new anchor is
  recomputed against the current mesh; the `stale` flag clears.

Why we don't auto-re-anchor: the nearest face on a re-topologized mesh
is *not* necessarily the artist's intended reference. The annotation
"soften this edge by 30%" placed on a specific seam might land on an
adjacent surface after re-topology — visually "close enough" but
semantically wrong. Forcing the reviewer to confirm preserves intent.

---

## The re-anchor sub-mode UX

Triggered by clicking `Re-place` on a stale annotation in the Review
Panel. Solarxy enters a sub-mode (the title bar changes to "Re-anchor
mode — click target triangle, Esc to cancel") and the next click on
model geometry re-anchors the annotation. Esc exits without changes.

Implementation:
- State lives on `ReviewState::reanchor_target: Option<String>` (the
  annotation ID being re-anchored).
- Click routing is documented in
  [`CLAUDE.md` § Review System click routing](../CLAUDE.md) and
  implemented in `state/input/mod.rs::try_review_pick`.
- The new anchor uses the same raycast primitive
  (`state/raycast.rs::raycast_scene`) as the initial placement, so
  the precision floor is identical.

---

## Sidecar file format reference

The on-disk JSON shape (format version 1):

```json
{
  "format_version": 1,
  "model_hash": "<hex>",
  "mesh_hashes": ["<hex>", "<hex>"],
  "annotations": [
    {
      "id": "01HF7Z3N9K...",                 // ULID
      "created_at": "2026-05-19T14:30:00Z",
      "updated_at": "2026-05-19T14:45:12Z",
      "author": "Alice <alice@example.com>", // optional, opt-in
      "anchor": {
        "mesh_index": 0,
        "face_index": 1234,
        "barycentric": [0.5, 0.3, 0.2],
        "world_pos_fallback": [1.2, 0.8, -0.3]
      },
      "category": "Question",                // Info | Warning | Question | Change
      "text": "Feels too sharp — soften by ~30%?",
      "reply_to": null,                      // ULID of parent for threaded replies
      "resolved": false,
      "stale": false
    }
  ]
}
```

### Field semantics

| Field | Purpose |
|---|---|
| `format_version` | Schema version. Always `1` in 0.6.0. Future-additive fields will not bump this; field removals or shape changes will. |
| `model_hash` | Stable hex hash of the model file at the time the review was first saved. Informational; not load-bearing for re-anchor logic. |
| `mesh_hashes` | Per-mesh stable hash, **positionally indexed** (`mesh_hashes[N]` is the hash for mesh N). Used at load time to detect topology drift on a per-mesh basis (the contract check above). |
| `annotations[].id` | ULID. Sortable, ~26 chars, no collisions in practice. Used by `reply_to`. |
| `annotations[].author` | Free-form. Opt-in: defaults to `null` (anonymous) unless the user opts in via the Preferences modal. Resolves to `git config user.name` if configured, otherwise OS username. |
| `annotations[].stale` | Set to `true` on load when the anchor fails the contract check. Cleared on successful re-anchor via the Review Panel. |
| `annotations[].reply_to` | ULID of parent annotation. Threading is one-level (replies to a reply attach to the same parent), which keeps the panel UI flat. |

### Sidecar discovery

Solarxy reads `<model>.solarxy-review.json` from the same directory as
the model by default. Override via `solarxy.toml`:

```toml
[review]
sidecar_dir = "reviews/"      # relative to model dir, or absolute
```

This is useful for studios that want to keep reviews in a parallel
review folder rather than alongside binary assets in Git LFS.

---

## Versioning + migration

The `format_version` field anchors forward compatibility. The rules:

- **Adding optional fields** does not bump the version. Readers must
  tolerate unknown fields (serde defaults handle this transparently).
- **Removing or renaming a field** bumps the version. A migration path
  is documented before the bump lands; old files load via a per-version
  upgrade shim.
- **Bumping the version without a documented migration path** is a
  workspace policy violation; CI enforces a check against the published
  baseline.

Vendors / downstream tooling that read the JSON directly should:
- Treat unknown fields as harmless (don't fail on them).
- Lock to a known `format_version` value and re-evaluate on bumps.
- Round-trip-test by reading + writing + reading and comparing
  semantically (string-equality fails on whitespace / key-order
  differences across `serde_json` versions).

---

## Privacy

- The `author` field is **opt-in**. On first save, Solarxy shows a
  modal asking whether to attach an author identity. The choice is
  remembered in `Preferences.review.author_opt_in`; subsequent saves
  honour it without re-prompting.
- The default author resolution is `git config user.name` if available
  (so authors who already publish their identity in Git aren't surprised),
  falling back to the OS username, then to `"anonymous"`.
- No telemetry, no remote logging, no cross-session author tracking.
  The review file is the single source of truth.

---

## What this contract does NOT promise

Out of scope for 0.6.0; flagged here so expectations are clear:

- **Cross-tool round-tripping** (export from Blender, re-import via Maya,
  re-export). The contract holds only when the same exporter is used
  on the same source file across rounds. Cross-tool workflows almost
  always re-topologize en route.
- **UV-space anchoring.** Annotations anchor in 3D space, not UV space.
  A 0.7.0+ task tracks "pin annotation to UV island" as a separate
  feature.
- **Multi-user concurrent editing.** Two reviewers editing the same
  `.solarxy-review.json` simultaneously will produce merge conflicts
  via Git, same as any other text file. The official answer is "use
  Git"; no Solarxy-side conflict-resolution UI is planned.
- **Annotation history / undo.** Each save replaces the file. If you
  want history, version the sidecar in Git alongside the model — Solarxy
  recommends this and is designed for it.

---

## See also

- [`solarxy-core/src/review.rs`](../crates/solarxy-core/src/review.rs) —
  the canonical Rust type definitions
- [`solarxy-app/src/state/raycast.rs`](../crates/solarxy-app/src/state/raycast.rs) —
  the CPU raycaster underpinning both placement and re-anchor
- [`CLAUDE.md` § Review System](../CLAUDE.md) — implementation
  architecture notes for maintainers
- [Solarxy Wiki / Review System][wiki-review] — user-facing
  walkthrough (categories, replies, panel filters)

[wiki-review]: https://github.com/marko-koljancic/solarxy/wiki/Review-System
