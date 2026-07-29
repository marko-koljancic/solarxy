# Label palette and taxonomy

Colors are the product's own pastels, so the public board reads as the same product as the
site. Sources, in order of authority:

- `web/src/landing/landing.css`, the Solarxy landing page tokens. This is the primary source
  and includes `--sage`, which the upstream editorial system does not define.
- The upstream editorial system in the separate site repo, for the `-soft` and `-deep`
  variants the landing page does not carry.
- Anything present in neither is marked **derived** below, with the rule used to derive it.

GitHub takes colors as six hex digits with no leading hash, and picks label text color by
luminance. **The palette is two-tier by design.** The `level:` and `area:` families are light
pastels carrying dark text, matching the product's on-pastel ink. The two signal colors are
deliberately deep so an alert does not read as another category: `d4623c` is the brand clay,
and `9a4a2e` is dark enough that GitHub renders **light** text on it. That contrast break is
the point, not an oversight.

**Color encodes family, not identity.** Within a family the colors are distinct, because that
is the dimension being scanned. Across families reuse is permitted and intentional, since the
prefix disambiguates.

## Family: level

Exactly one per issue. A lavender depth ramp, so the tree reads as depth at a glance.

| Label | Color | Token | Use |
|---|---|---|---|
| `level: milestone` | `9c90e0` | lavender-deep (upstream) | The milestone tracking issue. One per release. |
| `level: epic` | `c9c2f0` | `--lavender` (landing) | A named capability grouping tasks. |
| `level: task` | `e2def7` | lavender-soft (upstream) | One unit of work with its own acceptance criteria. |
| `level: subtask` | `ece7dc` | `--paper-sunken` (landing) | A split that earned its own issue under the granularity rule. |

## Family: area

At least one per issue. Distinct hues, because area is the main filter dimension.

| Label | Color | Token | Use |
|---|---|---|---|
| `area: engine` | `e4aa93` | `--coral` (landing) | Document, topology, cook, registry, undo, scene file. |
| `area: renderer` | `f2c9a0` | `--peach` (landing) | Render passes, shaders, pipelines, lighting, capture. |
| `area: web` | `b6d6bb` | `--sage` (landing) | The browser frontend, the wasm host, the public pages. |
| `area: cli` | `f4b59e` | coral-soft (upstream) | The command-line binary and its analysis output. |
| `area: desktop` | `f8e2cb` | peach-soft (upstream) | The native GUI shell and its panels. |
| `area: docs` | `d7e8da` | **derived** sage-soft | Wiki, reference documentation, planning docs, release notes. |
| `area: infra` | `d8d2c6` | `--hairline` (landing) | CI, packaging, release train, deploy, edge. |

Derivation for `d7e8da`: the upstream `-soft` variants lift their base toward paper by
roughly fifteen to twenty percent lightness at reduced saturation (`c9c2f0` to `e2def7`,
`e4aa93` to `f4b59e`, `f2c9a0` to `f8e2cb`). Applying the same lift to `--sage` `b6d6bb`
gives `d7e8da`. Replace it if the upstream system ever defines a real sage-soft.

## Family: signal

Applied only when true. The brand clay is reserved for the two that mean stop and look, so an
alert never competes with ordinary categorization. These are the deep tier: `d4623c` and
`9a4a2e` are darker than every categorization color, and `9a4a2e` crosses the threshold where
GitHub switches the label text to light. Both are intentional.

| Label | Color | Token | Use |
|---|---|---|---|
| `blocked` | `d4623c` | `--clay-strong` (landing) | Cannot proceed. The body must name what unblocks it. |
| `security` | `d4623c` | `--clay-strong` (landing) | Has a security dimension. Reviewed before close. |
| `bug` | `9a4a2e` | `--accent-ink` (landing) | Defect in shipped behavior, as against new work. |
| `good first issue` | `b6d6bb` | `--sage` (landing) | Self-contained, documented, no hidden context. |
| `help wanted` | `d7e8da` | derived sage-soft | Open to a contributor picking it up. |

## Retirements

These duplicate or shadow something that already exists. Relabel the issues first, then
delete the label.

| Retire | Because |
|---|---|
| `New Feature` | Superseded by `level:` plus `area:`. Overlaps `enhancement` and `type: feature`. |
| `type: feature` | Level and area carry this now; "not a bug" is the default. |
| `type: chore` | Use `area: infra` or `area: docs`. |
| `type: bug` | Superseded by the shorter `bug`. |
| `milestone: <version>` (all) | Shadows the native Milestone field, which is authoritative. |
| `enhancement`, `duplicate`, `invalid`, `question`, `wontfix`, `documentation` | GitHub defaults, unused or superseded. Close reasons and `area: docs` cover them. |

## Applying it

Create or recolor. `label create` fails if the label exists, so `edit` is the idempotent form:

```
gh label create "level: epic" --color c9c2f0 --description "A named capability grouping tasks"
gh label edit   "level: epic" --color c9c2f0
```

Retire, after relabelling every issue that carries it:

```
gh issue list --label "New Feature" --state all --json number --jq '.[].number'
gh label delete "New Feature" --yes
```

Verify the whole set, and confirm no color drifted from the source tokens:

```
gh label list --limit 100 --json name,color,description
grep -nE "^\s*--(lavender|coral|peach|sage|clay-strong|accent-ink|hairline|paper-sunken)" \
  web/src/landing/landing.css
```
