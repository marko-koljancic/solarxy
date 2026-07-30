---
name: solarxy-brand
description: "The design language and voice for Solarxy's public surfaces: the landing page, the roadmap page, and the references page. Carries the editorial token set, the type scale, layout and section patterns, the deep-link pattern, the tone guide, and the three platform constraints that make a new page work in production. Use before designing, writing, or building anything publicly visible on the site."
---

# Solarxy public surfaces

The app and the public pages deliberately use **different** design systems. The app uses its
own semantic role tokens; the public pages use the editorial system shared with the author's
main site. That is a considered divergence, not drift, and the most visible sign of it is that
the public dark mode is warm (`#16140f`) while the app's is neutral grey.

Everything here is public-facing, so the public-surface rules in
`.claude/skills/solarxy-domain/SKILL.md` apply in full, including redaction.

## Three platform constraints

Get these wrong and the page is broken in production while working perfectly in development.
They are the first thing to check, not the last.

1. **There is no router.** The frontend is a multi-page build, not a single-page app with
   routes. A new page means a new HTML entry plus a new input in the Vite config, **and** a
   new exact-match location in the edge config, which lives in a separate repository. Header
   directives do not inherit across locations, so any location that sets a header must
   restate the complete set.
2. **The fallback lies.** The vhost ends in a fallback that serves the landing page for any
   unmatched path, with status 200. A page whose route was never registered therefore looks
   like it works. **Verify by body or content length, never by status code.** This once hid a
   missing route for a whole release.
3. **The content security policy is self-only for scripts, styles, and fonts.** Fonts are
   self-hosted woff2 under `web/public/fonts/`, declared in `web/src/public/fonts.css`; a
   third-party font link or a data: URI is blocked in production and silently fine locally.
   Scripts have no unsafe-inline either, so a page carries zero inline script blocks and
   zero inline event handlers; all behavior lives in an external module entry.

The three public pages share `web/src/public/base.css` (tokens, shell, nav and footer
chrome, buttons, reveal) except the landing, which keeps its self-contained
`web/src/landing/landing.css`. A drift test pins both files' light values to the Rust
palette and asserts their dark values agree with each other, so changing them breaks the
build rather than just the design.

## Tokens

Light, and the authoritative set. `--sage` is Solarxy's own addition and is not in the
upstream system; everything else matches it verbatim.

```
--paper: #f4f1ea;        --paper-raised: #fbf9f4;   --paper-sunken: #ece7dc;
--ink: #11110e;          --ink-secondary: #4a463e;
--hairline: #d8d2c6;     --hairline-ink: rgba(17,17,14,.14);

--lavender: #c9c2f0;     --lavender-soft: #e2def7;  --lavender-deep: #9c90e0;
--coral:    #e4aa93;     --coral-soft:    #f4b59e;  --coral-deep:    #9a4a2e;
--peach:    #f2c9a0;     --peach-soft:    #f8e2cb;  --peach-deep:    #e3a86f;
--sage:     #b6d6bb;                                /* Solarxy only */

--clay-strong: #d4623c;  /* small non-text marks only, about 3.3:1 on cream */
--accent-ink:  #9a4a2e;  /* the AA-safe text accent on cream */
--focus:       #9a4a2e;
--violet-ink:  #5a4bb3;
--on-block:    #11110e;  /* blocks stay light in both themes, so this is fixed */
```

Dark, which is warm rather than grey:

```
--paper: #16140f;        --paper-raised: #201e17;   --paper-sunken: #100f0b;
--ink: #f2efe7;          --ink-secondary: #b4ad9e;  --hairline: #34302a;
--lavender: #bcb4ea;     --coral: #dca28b;          --peach: #e4bc8e;
--sage: #a7cbad;
--accent-ink: #e0875a;   --focus: #e0875a;          --clay-strong: #e06b43;
--violet-ink: #cabfff;
```

**Accent blocks stay light in both themes**, which is why the on-block ink is fixed dark.
Pastel fills are fills; only the deep clay and the deep violet are safe as text on paper.

## Type

Three families, each with one job. Space Grotesk for body and every heading, at weight 700 for
headings. Instrument Serif, italic, for a single accent word inside a heading, never for a
whole line. Space Mono for kickers, indexes, and metadata, uppercase with wide tracking.

```
--fs-display-xl: clamp(2.5rem, 8vw, 8rem)          --lh-display: .95    --ls-display: -.02em
--fs-display-l:  clamp(2.125rem, 6vw, 5rem)        --lh-display-l: 1    --ls-display-l: -.015em
--fs-h2:         clamp(2rem, 4vw, 3.25rem)         --lh-h2: 1.05        --ls-tight: -.01em
--fs-h3:         clamp(1.5rem, 2.5vw, 2rem)        --lh-h3: 1.1
--fs-body-l:     clamp(1.125rem, 1.5vw, 1.375rem)  --lh-body-l: 1.5
--fs-body:       1.0625rem                         --lh-body: 1.6
--fs-meta:       .8125rem   --fs-meta-sm: .75rem   --ls-meta: .08em
```

Spacing is an 8px base: `.5rem`, `1rem`, `1.5rem`, `2rem`, `3rem`, `4rem`, `6rem`, `8rem`.
Radius is `6px` for cards, `22px` for the soft variant, pill for pills. The shell reserves
`--nav-h: 4.5rem` and lifts with
`--shadow-lift: 0 18px 40px -24px rgba(17,17,14,.45)`.

## Layout and section patterns

- **Page shell.** A content container capped around 1440px with fluid edge padding,
  `clamp(1.25rem, 5vw, 6rem)`. Prose caps at 66ch regardless of container width.
- **Section opener.** A mono uppercase kicker, then a bold grotesque heading at `--fs-h2` with
  exactly one serif-italic accent word, then a secondary-ink subtitle at `--fs-body-l` capped
  to prose width. This rhythm is the single most recognizable thing about the system; keep it.
- **Accent blocks** carry pastel fills with fixed dark ink and a hairline inside. Use them to
  break long stretches of prose, not to decorate.
- **Motion** is opt-in and gated behind both a JavaScript-enabled flag and
  `prefers-reduced-motion: no-preference`. Reveals, kinetic headlines, and drawn rules all
  degrade to static.

## Deep links

Every section is addressable. The pattern, which the roadmap page in particular needs:

- Stable slugs as `id` on every h2 and h3, generated by the same slug algorithm used to build
  the table of contents so the two can never disagree.
- The heading text is wrapped in an anchor to its own id, styled to inherit color and carry no
  underline, so it reads as a plain heading but is a permalink.
- `html { scroll-behavior: smooth; scroll-padding-top: var(--nav-h); }` plus
  `scroll-margin-top: calc(var(--nav-h) + 1rem)` on headings, so an anchor jump does not land
  under the fixed nav. The roadmap page implements this pattern in full; treat it as the
  reference implementation for any future page.
- A sticky contents rail with an IntersectionObserver scroll-spy marking the active section,
  shown only when there are enough sections to warrant it.
- If a page has more than one view, prefix the view hash so it cannot collide with a section
  id.

## Voice

Plain, specific, and confident without selling. The reader is technical and will check claims.

- Lead with what becomes possible, not with what was built.
- Prefer a concrete number or a named behavior to an adjective. "76 node types" beats
  "comprehensive"; "opens in a browser with no install" beats "frictionless".
- Never claim a capability the product does not have today. On a roadmap page, label clearly
  what has shipped, what is committed, and what is exploratory, and never let the three blur.
- One serif-italic accent word per heading is the house flourish. Two is noise.
- The writing-style rules from the domain briefing apply: no emoji, no em or en dashes, no
  decorative arrows, no divider lines.
- Redaction applies. Describe capability in Solarxy's own terms; no reference checkout other
  than the Minimystix prototype is ever named on a public page.

## Responsiveness and accessibility

Fluid `clamp()` type and spacing rather than breakpoint jumps, with breakpoints reserved for
layout changes. Wide content, meaning tables, diagrams, and code blocks, scrolls inside its own
horizontally scrollable container; the page body never scrolls horizontally. Images cap at
full width. Both themes are styled, and contrast is checked in both: the pastels are fills, and
only the deep tokens are safe for text. Color never carries meaning alone. Focus is visible,
using the focus token.

## Upstream and drift

The token values above are copied so this file works when the upstream repository is not
present. Upstream is the main site repo's global stylesheet, plus
`web/src/landing/landing.css` and `web/src/public/base.css` for the Solarxy-specific
additions. All are authoritative over this file. Re-check them before any significant page
work, and if they have moved, update this file in the same pass rather than leaving two
versions of the truth.
