# Design sources

This directory holds the visual source of truth for each Solarxy shell. The design file leads
and the code follows: a visual change is designed here first, then implemented.

Paths below are relative to the repository root, matching the convention used throughout
`.claude/` and the planning documents.

| Surface | File | State |
|---|---|---|
| Web app and public pages | `design/web/solarxy-web.pen` | Populated. Fifteen design bands and a numbered decision series. |
| Desktop GUI (egui) | `design/desktop/solarxy-desktop.pen` | Populated. Fourteen bands and a decision series, split into two regions. **AS-IS** captures the shipped shell: tokens in both themes, typography, the widget state matrix, source-read metrics, the menu bar, status bar, pane toolbar, shell composition and the five viewport layouts, all seven dock panels, the modals, and the overlay and review surfaces. **PROPOSED** holds forward-looking work, currently the three panel changes the 0.8.2 desktop engine surface introduces. |
| Analyze TUI (ratatui) | `design/tui/solarxy-tui.pen` | Reserved. Created and intentionally empty. |

## Opening these files

`.pen` files are **Pencil** design files and they are **encrypted**. Open them only through the
Pencil MCP tools. Never `Read` them, never `Grep` them, and never open them with a text editor:
you will get ciphertext, not markup, and any edit written that way corrupts the file.

Start with `get_editor_state(include_schema: true)`. The schema is required before any other
Pencil tool call will make sense.

## What a design file owns, and what it does not

The split matters, because half of it is enforced by tests and half is not.

**Colour is owned by the code, not by the design file.** `solarxy_core::theme::Palette` in
`crates/solarxy-core/src/theme.rs` is the single colour source for all three shells:

- `crates/solarxy-app/src/gui/theme.rs` maps it onto egui for the desktop GUI.
- `crates/solarxy-cli/src/tui_theme.rs` maps it onto ratatui for the analyze TUI.
- `crates/solarxy-core/examples/gen_tokens.rs` generates `web/src/styles/tokens.generated.css`
  for the web app.

So a design file never invents a colour. Changing a colour means editing the palette in
`solarxy-core` and regenerating the web tokens with
`cargo run -p solarxy-core --example gen_tokens > web/src/styles/tokens.generated.css`.

This is enforced. `crates/solarxy-core/tests/tokens_drift.rs` carries
`generated_tokens_match_disk`, `hand_authored_css_does_not_redefine_generated_tokens`,
`every_css_var_resolves_to_a_defined_token`, and `landing_light_values_match_the_palette`.
A colour introduced anywhere other than the palette fails the build.

**Composition is owned by the design file.** Layout, spacing rhythm, component anatomy,
iconography, states, motion intent, and the relationships between screens live in the `.pen`
and nowhere else. None of that is machine-checked, so it relies on the file actually being
updated. That is the discipline this README exists to state.

The public marketing pages are a partial exception: they follow the editorial design language
documented in `.claude/skills/solarxy-brand/SKILL.md`, whose token set is deliberately separate
from the app's semantic roles. The `.pen` still owns their composition.

## When to update

Design leads implementation. Update the relevant `.pen` **before** the code change, not after,
so the file stays a source rather than a record. In practice:

- A new component, panel, or screen: design it in the `.pen` first.
- A change to an existing component's anatomy or states: update its band.
- A pure colour change: edit the palette instead, then regenerate. The `.pen` does not change.
- A behaviour change with no visual consequence: the `.pen` does not change.

Design decisions are numbered in the file's own decision series. Interaction changes also need
an amendment to the UX specification, per the amendment discipline in
`.claude/skills/solarxy-domain/SKILL.md`.

## The `.glsl` files

`design/web/checker.glsl`, `design/web/dots.glsl`, `design/web/slots.glsl`, and
`design/web/stripes.glsl` are Pencil shader
fills, authored for use as pattern surfaces inside the design file. They declare Pencil's
`@resolution` uniform convention. **They are design-side assets and are not compiled or loaded
by any application code**, which is why searching the source tree for them returns nothing. The
application's own shaders live in `crates/solarxy-renderer/src/shaders/`.

## Written companions

The `.pen` carries the visuals; the prose lives elsewhere:

- `.claude/skills/solarxy-brand/SKILL.md`, the design language and voice for the public pages.
- `../../Docs/Archive/SOLARXY-UX-SPEC.md`, personas, journeys, the interaction model, and the
  realtime UX contract.
- `../../Docs/Archive/SOLARXY-WEB-REVAMP.md`, the written companion to the web design work and the
  continuation of its decision series.

One code path references the design source directly: `web/src/flow/nodeVisual.ts` transplants
its glyph art from the web file's glyph set. If that art changes in the `.pen`, that file
changes with it.
