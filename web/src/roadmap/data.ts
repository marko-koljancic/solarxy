/* The hand-authored data module behind the public /roadmap page.
 *
 * This is the interactive twin of the internal roadmap document. Nothing
 * generates it and nothing validates its content against the docs, so a doc
 * change desynchronizes it silently; the cross-surface sync contract names the
 * arrays below and says which doc change touches which one. Because every
 * string here renders on a public page, the redaction rule applies in full:
 * no reference-checkout names, no internal doc paths, no planning codes.
 * data.redaction.test.ts sweeps this file's source text for all three.
 *
 * Counts are single-sourced in the consts below; STATS, HERO_CHIPS and
 * FOOTER_META derive from them so the numbers cannot disagree with each
 * other. counts.test.ts pins NODE_TYPE_COUNT to the landing page stats band.
 */

export const NODE_TYPE_COUNT = 77;
export const CRATE_COUNT = 15;
export const CONTEXT_COUNT = 4;
export const SHELL_COUNT = 3;
export const VALIDATION_KIND_COUNT = 11;
export const CARD_COUNT = 72;
export const PROGRAM_RELEASE_COUNT = 13;
export const RELEASES_TO_1_0 = 3;
export const JOURNEY_COUNT = 16;
export const LIVE_VERSION = "v0.9.0";

export interface Section {
  id: string;
  label: string;
}

/* The one list that drives both the section ids in the HTML shell and the nav
 * table of contents, so the two can never disagree. Slugs are public URLs:
 * stable forever once shipped. */
export const SECTIONS: Section[] = [
  { id: "overview", label: "Overview" },
  { id: "architecture", label: "Architecture" },
  { id: "shipped", label: "Shipped" },
  { id: "changelog", label: "Changelog" },
  { id: "releaseplan", label: "To 1.0" },
  { id: "program", label: "The program" },
  { id: "personas", label: "Personas" },
  { id: "workflows", label: "Workflows" },
  { id: "coverage", label: "Coverage" },
  { id: "explorer", label: "Explorer" },
];

export interface Stat {
  n: string;
  l: string;
}

export const STATS: Stat[] = [
  { n: String(NODE_TYPE_COUNT), l: "Node types, test-enforced" },
  { n: String(CRATE_COUNT), l: "Workspace crates" },
  { n: String(CONTEXT_COUNT), l: "Typed contexts: Obj, Geo, Mat, Tex" },
  { n: String(SHELL_COUNT), l: "Shells: desktop, CLI, web" },
  { n: String(VALIDATION_KIND_COUNT), l: "Validation kinds" },
  { n: String(CARD_COUNT), l: "Roadmap cards, all dispositioned" },
  { n: String(PROGRAM_RELEASE_COUNT), l: "Releases in the numbered program" },
  { n: LIVE_VERSION, l: "Live on solarxy.koljam.com" },
];

export const PROGRAM_STATS: Stat[] = [
  { n: String(CARD_COUNT), l: "Roadmap cards, all dispositioned" },
  { n: String(PROGRAM_RELEASE_COUNT), l: "Releases in the numbered program" },
  { n: String(RELEASES_TO_1_0), l: "Releases to 1.0" },
  { n: String(JOURNEY_COUNT), l: "User journeys covered" },
];

export const HERO_CHIPS: string[] = [
  `${NODE_TYPE_COUNT} node types`,
  `${CRATE_COUNT} crates`,
  "One Rust core, three shells",
  `${LIVE_VERSION} live`,
];

export const FOOTER_META: string[] = [
  "Updated August 2026",
  `${LIVE_VERSION}, live on solarxy.koljam.com`,
  `${NODE_TYPE_COUNT} node types, ${CRATE_COUNT} crates`,
];

export interface ArchCrate {
  n: string;
  d: string;
}

export interface ArchLayer {
  tag: string;
  desc: string;
  cls: string;
  crates: ArchCrate[];
}

export const ARCH_LAYERS: ArchLayer[] = [
  {
    tag: "Data core",
    desc: "GPU-free types and IO, wasm-clean",
    cls: "",
    crates: [
      {
        n: "solarxy-core",
        d: "Geometry, validation, preferences, the scene contract (SceneDelta / SceneOp), raycast, theme.",
      },
      {
        n: "solarxy-formats",
        d: "OBJ / STL / PLY / glTF loaders and exporters; byte-first, wasm-clean.",
      },
      { n: "solarxy-imaging", d: "Pure-CPU image operators for the texture context." },
    ],
  },
  {
    tag: "Engine",
    desc: "The headless studio core; never touches the GPU",
    cls: "engine",
    crates: [
      {
        n: "solarxy-kernel",
        d: "Pure-CPU parametric geometry: GeometrySet / KernelMesh, 7 primitives, transform / merge.",
      },
      {
        n: "solarxy-graph",
        d: "Document, topology, cook engine, registry, undo, review, plus the Engine facade.",
      },
      {
        n: "solarxy-scenefile",
        d: "The .slxy ZIP format: serde schema, content-addressed assets, migration gate.",
      },
      { n: "solarxy-validate", d: "Validation orchestration and CI pipeline adapters." },
    ],
  },
  {
    tag: "Renderer",
    desc: "All wgpu state; compiles to wasm",
    cls: "render",
    crates: [
      {
        n: "solarxy-renderer",
        d: "Pipelines, IBL, SSAO, bloom, shadow, composite. Talks to the engine only through SceneDelta.",
      },
    ],
  },
  {
    tag: "Shells",
    desc: "Three consumers of the one core",
    cls: "shell",
    crates: [
      { n: "solarxy (root)", d: "Thin always-GUI entrypoint; parses GuiArgs, calls run_viewer." },
      {
        n: "solarxy-app",
        d: "winit + egui desktop shell: viewer, validator, reviewer. Not yet wired to solarxy-graph.",
      },
      {
        n: "solarxy-web",
        d: "wasm-bindgen boundary + WebGPU host driving the full renderer (cdylib).",
      },
      {
        n: "solarxy-host",
        d: "The orchestration both GPU shells share: pane render loop, lighting chokepoint, view state, gizmo solver.",
      },
      {
        n: "solarxy-cli",
        d: "clap args + analyzer + the analyze surface: a tiled terminal workspace over capability tiers, file-based themes, a split tree and a braille rasteriser.",
      },
      {
        n: "web/ (React 19)",
        d: "Vite + @xyflow/react display mirror. Not a crate; the frontend.",
      },
    ],
  },
];

export const CONTRACTS: Stat[] = [
  { n: "4", l: "typed contexts, Obj / Geo / Mat / Tex, with cross-context path references" },
  { n: "13", l: "wire DataTypes and a snapshot-tested 13x13 coercion matrix; no Any / Object" },
  { n: "1", l: "frozen schema_version; every node change rides a type_version bump + migration" },
  { n: "0", l: "frontend changes to add a Rust node; the registry snapshot drives the UI" },
];

export interface Commitment {
  h: string;
  p: string;
}

export const COMMITMENTS: Commitment[] = [
  {
    h: "One core, two shells",
    p: "The engine (solarxy-graph) and renderer (solarxy-renderer) never depend on each other; they talk through SceneDelta in solarxy-core, and on web both compile into one wasm instance so cooked geometry never crosses into JavaScript.",
  },
  {
    h: "Renderer refactors land desktop-first",
    p: "winit decoupling, byte-fed loading, async readbacks, multi-object scenes, a generalized 8-light array. Desktop stays regression-free at every step.",
  },
  {
    h: "The frontend is a display mirror",
    p: "Rust owns all document state; React mirrors it via EngineEvent batches and mutates only by dispatching Commands. A node added in Rust needs zero frontend changes; a new ParamType / DataType is a deliberate exception.",
  },
  {
    h: "The two shells have different jobs",
    p: "Settled 2026-07-28. The engine is shared without exception. Desktop is the professional and pipeline surface: threaded cooking, no capture ceiling, hardware ray tracing, a real filesystem, multi-window, headless runs. Web is the reach surface: zero-install, share links, and the standalone bundle publish. Parity is a promise about capability, not chrome.",
  },
  {
    h: "Minimystix is a spec, not code",
    p: "Its Three.js renderer is discarded (the wgpu renderer is a strict superset); its engine semantics port to Rust with the vitest suites re-expressed as Rust tests; its React UI is copied and rewired to mirror-and-command.",
  },
];

export interface TimelineEntry {
  date: string;
  h: string;
  body: string;
}

export const TIMELINE: TimelineEntry[] = [
  {
    date: "2026-03, Act I",
    h: "v0.1.0 First Light",
    body: "The first public tag: a Rust and wgpu engine with OBJ loading, Cook-Torrance PBR, an in-viewport HUD, and the analyze TUI. Act I (pre-0.3) was about building the foundation: get the renderer, formats, and inspection core solid and quiet. No launch, no noise, just a tool worth showing.",
  },
  {
    date: "2026-04, Act I",
    h: "v0.2.0 Interactivity",
    body: "Grid and gizmo, bounding box, turntable, bloom, background presets, settings persistence, and side-by-side compare.",
  },
  {
    date: "2026-04, Act II",
    h: "v0.3.0 Feature Baseline",
    body: "Four operation modes, per-material PBR, procedural and HDRI image-based lighting, environment reflections, SSAO, and ACES tone mapping. Act II (v0.3 to v0.4) was the screenshot moment: inspection overlays and the split viewport make a single image that explains the whole product.",
  },
  {
    date: "2026-04, Act II",
    h: "v0.4.0 Inspection Intelligence",
    body: "The pivot from viewer to debugger: egui, split viewports, the inspection modes, the validation overlay, and the workspace split into crates.",
  },
  {
    date: "2026-04, Act III",
    h: "v0.5.0 Two Binaries",
    body: "GUI and CLI split into separate binaries, native installers, a Preferences dialog, and grouped sidebar controls. Act III (v0.5 to v0.6) drove pipeline adoption: the review system, configurable validation, and a 30-second CI setup turn individual interest into recurring team use.",
  },
  {
    date: "2026-05, Act III",
    h: "v0.6.0 Studio Adoption",
    body: "Spatially-anchored review notes, dockable panels, a Material Inspector, overdraw and AO modes, configurable validation, CI adapters, and Homebrew plus winget.",
  },
  {
    date: "2026-07, The web pivot",
    h: "v0.7.0 Web Reach",
    body: "The browser build, ahead of its target: Solarxy Web on WASM and WebGPU at solarxy.koljam.com, with node-based parametric modeling on the same Rust core, the .slxy scene format, and transform gizmos. The engineering waves that produced it and v0.7.1 are broken out below.",
  },
  {
    date: "2026-07-07 to 07-11",
    h: "The integration wave",
    body: "The WebGPU spike, desktop-first renderer decoupling (byte-fed loading, async readbacks) and the multi-object scene, the headless engine (Command in, EventBatch out), the web-shell MVP proving the zero-frontend-change contract, imports and assets and the .slxy format, viewer-systems parity, and a review-and-polish pass.",
  },
  {
    date: "2026-07-11 to 07-15",
    h: "The expansion wave",
    body: "Correctness and wiring, shell reorganization (each pane owns its menus), free pane docking via dockview, the translate / rotate / scale gizmos with snapping and light helpers, textures end-to-end and the Image DataType, the material system, and the modeling wave: array, mirror, null, switch, bounds, delete, uv_project, subdivide.",
  },
  {
    date: "2026-07-15",
    h: "The UI revamp",
    body: "A whole-app restyle: the two-tier semantic token architecture, Inter + IBM Plex Mono self-hosted, registry-driven glyph and role node identity (the one sanctioned boundary change), a single disciplined motion system (90 / 160 / 240 ms), zoom-responsive node LOD, and a WCAG-AA accessibility bar.",
  },
  {
    date: "2026-07-16",
    h: "The viewport and presentation wave",
    body: "Ten items that made the web app a real 3D application: root-level camera nodes (perspective / orthographic / physical), an asset preview pane, Blender-style transform tool keys, per-pane pastel header colors, grid-transform fixes, temporary shading overrides, preference persistence, turntable spin and export (WebM / MP4 / PNG-sequence), and the public landing page.",
  },
  {
    date: "2026-07-16",
    h: "The context expansion",
    body: "Generalized the context model into four typed contexts (Obj / Geo / Mat / Tex), added cross-context path references, the texture and material contexts, the Material wire type, the jump-flood selection outline, and export / render nodes. The registry grew 34 to 58 types; the schema froze at 1 with contexts included.",
  },
  {
    date: "2026-07-17",
    h: "v0.7.1 release wave",
    body: "Hardening and consistency across desktop and web: one palette driving all three shells, all 58 node docs rewritten (342/342 params, 90/90 ports), the Node Reference regenerated 33 to 58, Rust tests 722 to 764 and vitest 110 to 128, wasm 1.40 MB gzip against a 2.5 MiB budget. Released and live at solarxy.koljam.com.",
  },
  {
    date: "2026-07-21",
    h: "v0.7.2 Polish",
    body: "The first point release on the road to 1.0: an ambient-occlusion correctness fix plus an AO strength control, a validate-node UV guard, four new CPU procedural texture generators (voronoi / gradient / checker / brick), parameter-panel dimension polish, and a preview worker-split. Registry 58 to 62; the best proof-by-repetition of the zero-frontend-change contract.",
  },
  {
    date: "2026-07-22 to 07-24",
    h: "v0.8.0 First Modeling Wave",
    body: "The points, curves, and attributes foundation lands: the kernel and renderer gain point clouds, poly-lines, and a typed per-element attribute system, unblocking the modeling wave. Scatter, copy_to_points, line, circle, points_from_geo, edges_to_geo and the attribute nodes join the catalogue; vertex colors run end to end with colored and face-less PLY import; geo_export writes materials to GLB and OBJ + MTL. Five maintainer feedback waves then carried it well past the plan: the Attributes pane, GPU-drawn attribute labels, a scene Tree panel, three new desks, panel maximize, Max-style view keys, and a projection-correct pick ray that fixed gizmos in axis views. Registry 58 to 74; the schema stays frozen at 1. Released 2026-07-22.",
  },
  {
    date: "2026-07-24 to 07-28",
    h: "v0.8.1 Expressions, Runtime and Publishing",
    body: "Expressions and the runtime ship together. Any numeric parameter can hold a formula: arithmetic, around thirty builtins, ch() references to another node, and geometry queries against its own inputs, with cycles refused at set time and a rename that rewrites every referring expression inside the rename's own undo step. That needed a naming model the codebase never had, so nodes now mint graph-unique auto-numbered names. The attribute wrangle runs the same language per point or primitive through one parser switched by a Scope trait, not a second grammar. A scene clock makes both move, with one tick per frame so $T is exactly $F / $FPS, dirtying only the nodes that read time. The File menu's Export web bundle writes a self-contained archive that carries the engine rather than a recording. And rect_area_light stops approximating, shading through linearly transformed cosines. Registry 74 to 76; the schema stays frozen at 1. All of it web-only: the desktop shell is still unwired from the node engine.",
  },
  {
    date: "2026-07-29 to 08-06",
    h: "v0.8.2 Rendering foundations",
    body: "The renderer closes five nameable gaps and the desktop gains its first engine surface. Principled surface parameters run end to end: transmission, clearcoat, sheen, iridescence and anisotropy import from glTF, shade in the viewport through one uber-shader with uniform branching, and export back with the extensions preserved. The environment becomes scene data: a float image type, HDR and EXR decode, and an environment node so the HDRI you light with saves inside the scene. Instancing becomes real: scatters and arrays carry transforms instead of baking copies, the raster path issues instanced draws, and every geometry node either carries placements or bakes them deliberately, never silently. The camera owns the look: exposure, lift, gamma and gain plus two LUT slots, one log-space before tone mapping and one after, travelling with the camera through save and load; light intensity moves to physical units with a migration that keeps old scenes looking identical. Under it all, the pane orchestration both shells had duplicated collapses into one shared host crate, verified by zero changed golden pixels, and on top of it the desktop opens .slxy scenes, renders them with no file loaded, and lists the graph in a read-only Node Tree. The analyze terminal report becomes a tiled workspace: panels you arrange yourself, four colour tiers, file-based themes, a braille model silhouette and UV occupancy map, and the triangle budget, issue kinds and mesh names the analyzer used to discard. The node canvas lands on one geometric contract: one layout box for every role, root nodes as fixture pills told apart by pastel, glyph and label, and the validation report becomes deterministic. Registry 76 to 77; the schema stays frozen at 1.",
  },
];

export interface ChangelogItem {
  lead: string;
  text: string;
}

export interface ChangelogGroup {
  h: string;
  items: ChangelogItem[];
}

export interface ChangelogEntry {
  v: string;
  code: string;
  date: string;
  status: "live" | "done" | "draft";
  statusLabel: string;
  open?: boolean;
  summary: string;
  groups: ChangelogGroup[];
  meta?: [string, string][];
}

export const CHANGELOG: ChangelogEntry[] = [
  {
    v: "v0.9.0",
    code: "Path-traced rendering",
    date: "September 2026",
    status: "live",
    statusLabel: "Released",
    open: true,
    summary:
      "Solarxy grows a second renderer. The viewport draws each pixel once and approximates where the light came from; the new one follows light through the scene and lets it bounce, so colour bleeds off a wall onto a floor, a small light casts a soft-edged shadow, and glass bends what is behind it. The command line becomes a first-class render surface at the same time, with exit codes a build system can branch on. A still can also leave with nothing behind it, as an element to composite rather than a finished picture, and lights stop being reachable only through the parameter panel: they are drawn in the viewport, and you can grab them.",
    groups: [
      {
        h: "A second renderer",
        items: [
          {
            lead: "Global illumination, computed rather than approximated.",
            text: "Light that bounced off something else before reaching a surface is followed rather than guessed at. That is what produces colour bleeding, and it is the difference the bundled Cornell Box sample exists to show.",
          },
          {
            lead: "Soft shadows with the right shape, and traced depth of field.",
            text: "A shadow's softness comes from the size of the light that cast it, so a rect area light is the one to reach for. Set a camera's F-Stop above zero and the aperture is integrated properly; a Focus Distance of zero focuses on whatever the camera is aimed at, so aiming also focuses.",
          },
          {
            lead: "Every light in the scene, and an edge-aware denoiser.",
            text: "The viewport binds eight lights; a traced render reads them all. The denoiser steers by the albedo and normal recorded while tracing, so it removes noise without smearing across the edge between two surfaces.",
          },
        ],
      },
      {
        h: "Rendering a still",
        items: [
          {
            lead: "The picture refines while it renders.",
            text: "It arrives in tiles and improves from the first few chunks rather than appearing at the end, so you can tell early whether the shot is the one you wanted. Pan and zoom it while it converges; the tiles keep landing in the right places underneath.",
          },
          {
            lead: "Albedo, normal and depth are selectable in the browser.",
            text: "Switch pass while the render is still running. A pass nobody asked for says what would produce it, and a rasterized render offers the beauty alone rather than three empty rows, because it has nothing else to give.",
          },
          {
            lead: "PNG or 32-bit float EXR, from every shell.",
            text: "In either the scene-linear or the display-referred space. The format is chosen before the render starts, because it decides what the renderer reads back.",
          },
        ],
      },
      {
        h: "Rendering with nothing behind it",
        items: [
          {
            lead: "A real matte, not a colour key.",
            text: "Opaque where the camera found a surface, clear where it found sky, fractional along every silhouette so an edge antialiases instead of staircasing. A mirror against the sky is opaque, because the camera did find a surface there. Both renderers honour it, and it survives exposure, the tone map and the grade.",
          },
          {
            lead: "Each format carries its own stated convention.",
            text: "Floating-point files carry premultiplied alpha and eight-bit files carry straight alpha, so a compositor that honours each composites both identically over the same plate. A matte whose convention is unstated is one somebody composites wrong.",
          },
        ],
      },
      {
        h: "Lights in the viewport",
        items: [
          {
            lead: "Every light draws a marker you can click.",
            text: "Screen-constant, so one across the scene is no harder to hit than one in front of you, with a shape that says which of the six kinds it is. Selecting it is the same selection the rest of the application means. Markers switch off per pane and never appear in a rendered still.",
          },
          {
            lead: "A light takes the transform tools, and offers only what it can use.",
            text: "The scene relights continuously as you drag, and a drag is one undo step. A fifth tool, Aim, moves the point a light points at rather than the light. A point light offers Move; a rect-area panel offers Move, Rotate and Scale, its size handles writing Width and Height in metres; the two lights with no position offer none.",
          },
        ],
      },
      {
        h: "Controls for the shot you are making",
        items: [
          {
            lead: "Named output sizes, and an exact sample count.",
            text: "HD, UHD 4K and 8K, the DCI sizes, square and 5:4, and A4, A3 and Letter at 300 dpi, with an orientation that turns them. Every entry states its own pixel size. The four quality presets keep the wide steps most shots want, with an exact count beside them for the scene that is not there.",
          },
          {
            lead: "A bright sample limit, a seed, and a steerable denoiser.",
            text: "The limit clamps the rare very bright indirect sample that arrives as a lone white speckle. The seed makes a render repeat exactly on the surface that produced it. Six steering values let the filter be tuned to a scene rather than accepted as it comes.",
          },
        ],
      },
      {
        h: "The command line renders",
        items: [
          {
            lead: "A render becomes a pipeline step rather than something a person does.",
            text: "Everything the render needs comes from the scene's own render node, with flags overriding one value at a time. Eight meaningful exit codes, and a flag that cannot take effect is refused rather than ignored.",
          },
          {
            lead: "Standard output is data.",
            text: "Progress goes to standard error, so the image can go down a pipe and a machine-readable result can be read by the next step. Watch it converge on a terminal dashboard, or in a window.",
          },
        ],
      },
      {
        h: "A licence change",
        items: [
          {
            lead: "From this release Solarxy is GPL-3.0-or-later.",
            text: "Releases through v0.8.2 were published under MIT and stay MIT for anyone holding them: that grant cannot be withdrawn, so this is a boundary rather than a retroactive change. An additional permission under section 7 covers the graph-layout library and the bundled fonts, so no feature was removed to reach compatibility.",
          },
        ],
      },
    ],
    meta: [
      ["Registry", "stays 77"],
      ["Schema", "stays 1"],
      ["Licence", "MIT to GPL-3.0-or-later"],
    ],
  },
  {
    v: "v0.8.2",
    code: "Rendering foundations",
    date: "August 2026",
    status: "live",
    statusLabel: "Released",
    open: true,
    summary:
      "Five nameable gaps in the renderer close in one release, the desktop gains its first engine surface on a newly shared host, and the analyze terminal report becomes a tiled workspace. The theme is foundations: the material model, the environment, instancing and the look pipeline are exactly what the path tracer consumes next release, laid down here so it renders scenes rather than approximations.",
    groups: [
      {
        h: "Principled surfaces",
        items: [
          {
            lead: "The extended material model runs end to end.",
            text: "Transmission, clearcoat, sheen, iridescence and anisotropy import from glTF, shade in the viewport, and export back with their extensions preserved, so a vendor file loses nothing on a round trip. One uber-shader with uniform branching keeps the pipeline count at one.",
          },
          {
            lead: "Texture maps for the new properties round-trip without shading.",
            text: "The core WebGPU sampled-texture budget is spent; the maps survive import and export and the parameter help says exactly that. The path tracer consumes the same data next release.",
          },
        ],
      },
      {
        h: "The scene owns its light",
        items: [
          {
            lead: "An environment node.",
            text: "The HDRI you light with becomes scene data: a float image type, Radiance and OpenEXR decode, and an environment node whose image saves inside the .slxy archive, so a scene reloads lit exactly as it was authored.",
          },
          {
            lead: "Physical light intensity.",
            text: "Light nodes move to physical units with a versioned migration and a matched viewer-rig rescale, proven by golden captures that did not change by a pixel.",
          },
        ],
      },
      {
        h: "Real instancing",
        items: [
          {
            lead: "Scatters carry transforms instead of baking copies.",
            text: "A ten-thousand-copy scatter stays interactive and issues instanced draws; Bake mode still collapses to real geometry when a downstream edit needs it, and old scenes pin to Bake so nothing changes behind their back.",
          },
          {
            lead: "Every node is on one side of a contract.",
            text: "A geometry node either carries placements through or bakes them first with a warning naming the count. Losing every copy but one silently is no longer a reachable state, and the export path bakes exactly where it encodes.",
          },
        ],
      },
      {
        h: "The camera owns the look",
        items: [
          {
            lead: "Colour grading travels with the camera.",
            text: "Exposure, lift, gamma and gain plus two LUT slots for .cube tables, one log-space before tone mapping and one display-referred after, all saved on the camera node so a graded scene reloads graded.",
          },
        ],
      },
      {
        h: "One host, two shells",
        items: [
          {
            lead: "The duplicated pane orchestration collapses.",
            text: "The per-pane render loop, lighting chokepoint, camera lifecycle and gizmo drag solver both shells carried now exist once in a shared host crate, and the golden harness renders through the same path it gates. Verified by zero changed pixels across every capture.",
          },
          {
            lead: "The desktop opens scenes.",
            text: "The desktop app opens a .slxy authored on the web, from the dialog or the launch argument, renders it identically with the engine cooking, lists the graph in a read-only Node Tree, and sources its Properties and Outliner panels from the engine scene. The editing canvas is the release after next.",
          },
        ],
      },
      {
        h: "The terminal workspace",
        items: [
          {
            lead: "The analyze report becomes a dashboard.",
            text: "Ten panel types tile the terminal: presets, free arrangement, maximize, per-panel selection and scrolling. Four colour tiers degrade gracefully to plain ASCII, themes come from files with a contrast floor, and a braille rasteriser draws the model silhouette and UV occupancy in the terminal.",
          },
          {
            lead: "The discarded facts surface.",
            text: "The triangle budget, per-kind issue counts, real mesh names and degenerate-face lists the analyzer always computed now reach the workspace and the plain text report.",
          },
        ],
      },
      {
        h: "One geometric contract",
        items: [
          {
            lead: "The node canvas simplifies.",
            text: "Every role occupies one layout box, so handles, wires and auto-layout align across silhouettes; root nodes render as fixture pills told apart by pastel, glyph and label; glyphs ink directly on the body. The public roadmap page stopped advertising unreachable documents and works on a phone.",
          },
          {
            lead: "The validation report becomes deterministic.",
            text: "The same build validating the same file now names the same issues in the same order, which the CI golden gate had quietly depended on all along.",
          },
        ],
      },
    ],
    meta: [
      ["Registry", "76 to 77"],
      ["Schema", "stays 1"],
    ],
  },
  {
    v: "v0.8.1",
    code: "Expressions, Runtime and Publishing",
    date: "July 2026",
    status: "live",
    statusLabel: "Released",
    open: true,
    summary:
      "Two foundational capabilities, shipped together on the reasoning that they are one story rather than three: an expression engine without a clock computes only static relationships, a clock without expressions has nothing to drive, and both stay invisible to anyone but the author until a scene can leave the editor. Supersedes the earlier plan that spread the three across separate releases. All of it is Solarxy Web only: the desktop shell is still unwired from the node engine.",
    groups: [
      {
        h: "De-risking first",
        items: [
          {
            lead: "Two timeboxed spikes before any feature work.",
            text: "The world-space light-loop hoist is measured against the golden captures at tolerance zero with no area-light code present, so a real shading regression cannot hide inside an expected re-baseline. The parameter dependency graph is measured on a synthetic 200-node document before its design is fixed.",
          },
        ],
      },
      {
        h: "Expressions",
        items: [
          {
            lead: "Turn on the reserved seam.",
            text: "The expression parameter source has existed since the beta, refused at a single chokepoint. A sandboxed grammar replaces the refusal: arithmetic, comparison, logic, a ternary, vector member access, and around thirty builtins.",
          },
          {
            lead: "Cross-node references.",
            text: 'ch("/geo1/sphere1/radius") and ch("../scatter1/count"), plus geometry queries (npoints, nprims, bbox, centroid) against the node\'s own inputs. A reference reads a parameter, which is document state rather than cook output, so it needs no change to the wire topology.',
          },
          {
            lead: "Renames do not break references.",
            text: "A rename rewrites every referencing expression inside the rename's own undo step, keeping the promise the id-backed node reference already makes.",
          },
          {
            lead: "Numeric and Bool params only.",
            text: "Text, enum, asset and node-path params stay literal-only and show no affordance.",
          },
          {
            lead: "An inline editor.",
            text: "An equals affordance on every eligible parameter row, a monospace expression field, a resolved-value readout, and a span-highlighted parser error.",
          },
        ],
      },
      {
        h: "Attribute wrangle",
        items: [
          {
            lead: "A per-element program.",
            text: "Multi-statement, with @P, @N, @Cd, @uv, @ptnum and any custom lane in scope, typed locals, and lane creation with an inferred type. No control flow in v1, deliberately.",
          },
          {
            lead: "A new parameter widget.",
            text: "A snippet parameter type stores plain text and declares a code editor, following the precedent set by the attribute-name field, so documents need no migration.",
          },
        ],
      },
      {
        h: "Runtime foundation",
        items: [
          {
            lead: "A scene clock and transport.",
            text: "Play, pause, stop, step, frame range and fps, owned by the document and saved with it behind a defaulted section, so the schema stays frozen at 1.",
          },
          {
            lead: "A tick that costs nothing when unused.",
            text: "Only nodes whose parameters carry a time-referencing expression are dirtied, so a scene without time expressions pays no per-frame price.",
          },
          {
            lead: "Foundation only.",
            text: "No event nodes, no actor graph, no keyframe channels; the runtime gets exactly one consumer so its design can be judged against a real use.",
          },
        ],
      },
      {
        h: "Standalone web export",
        items: [
          {
            lead: "A scene leaves the editor.",
            text: "An export action writes a self-contained archive: a player shell, the same wasm the editor runs, the scene, and a config for autoplay, loop, transport visibility, background and camera.",
          },
          {
            lead: "It plays.",
            text: "Time-driven expressions run in the exported bundle, which is what makes the runtime visible to an end user rather than only to the author.",
          },
          {
            lead: "One documented limitation.",
            text: "The bundle must be served over HTTP, not opened from a file URL.",
          },
        ],
      },
      {
        h: "Physically based area lights",
        items: [
          {
            lead: "Rect-area lights stop lying.",
            text: "Linearly-transformed-cosine shading replaces the point-light approximation, so width, height and orientation finally reach the shading. The node's own help text has been an apology for this since the second version of its descriptor.",
          },
          {
            lead: "The lighting loop moves to world space,",
            text: "which the golden captures adjudicate first, on its own commit.",
          },
        ],
      },
      {
        h: "An honest note on desktop",
        items: [
          {
            lead: "Four of the five features are web-only,",
            text: "because the desktop shell is still not wired to the node engine. Area lights look like the exception and are not: the desktop viewer has no light nodes at all, so it receives the shading change with no way to author an area light.",
          },
        ],
      },
    ],
    meta: [
      ["Registry", "74 to 76"],
      ["Schema", "stays 1"],
    ],
  },
  {
    v: "v0.8.0",
    code: "The first modeling wave",
    date: "July 2026",
    status: "live",
    statusLabel: "Released",
    open: true,
    summary:
      "Points become first-class. Geometry now carries named attributes on points and primitives, the registry grows from 58 to 74 node types, and the viewport learns to draw point clouds and polylines. Scatter points over any surface, stamp copies onto them, randomize a color per point, and watch it render, then inspect every value in the new Attributes pane. Released 2026-07-22.",
    groups: [
      {
        h: "Points, lines, and attributes",
        items: [
          {
            lead: "Point clouds and polylines render",
            text: "as camera-facing sprites and hardware lines beside the triangle meshes, with per-point vertex colors end to end through import, cook, display and export.",
          },
          {
            lead: "Attribute nodes.",
            text: "attribute_create writes a constant lane; attribute_randomize fills one with seeded uniform values (its default paints color, so anything wired through it displays per-point color immediately); attribute_promote moves lanes between the point and primitive domains; attribute_copy transfers a lane between geometries; attribute_from_image samples an image by UV.",
          },
          {
            lead: "The modeling wave.",
            text: "scatter, copy_to_points, points_from_geo, edges_to_geo, mirror, delete, line, circle and displace join the catalogue. The line node takes two optional geometry inputs: connect anything and its first point pins that end.",
          },
          {
            lead: "Color-aware import and export.",
            text: "PLY and glTF vertex colors survive the round trip; geo_export writes materials (OBJ plus MTL as a zip, GLB with the full PBR table and embedded textures) and point and line primitive modes.",
          },
        ],
      },
      {
        h: "See your data",
        items: [
          {
            lead: "The Attributes pane,",
            text: "a read-only spreadsheet over the selected node: point and primitive tabs, every lane in columns, virtualized so a hundred-thousand-point scatter scrolls smoothly.",
          },
          {
            lead: "Attribute visualization in the viewport,",
            text: "with per-point value labels, point numbers and vector arrows. Labels draw on the GPU as signed-distance-field glyphs straight from the renderer, so every point gets a label by default up to a 16,384 budget (denser scenes sample and say so), tracking the camera at zero per-frame CPU cost. Five curated color ramps and vector controls in the settings popover.",
          },
        ],
      },
      {
        h: "A viewport that behaves",
        items: [
          {
            lead: "View switching fixed end to end.",
            text: "The Persp / Ortho label always tells the truth, orbiting right after a view preset works instead of dying, orbiting out of a Top view tilts smoothly instead of lurching, and the reference grid switches planes once per view change instead of flickering mid-animation.",
          },
          {
            lead: "Max-style view keys",
            text: "over the viewport: T top, F front, L left, B bottom, P perspective, O orthographic, Z fit. Over the node pane, F fits the graph.",
          },
          {
            lead: "Transform gizmos work in axis views.",
            text: "The picking ray is now projection-correct, so dragging in-plane arrows and planes moves objects in Front, Top and Left views, and picking and review anchoring are exact under orthographic cameras everywhere.",
          },
          {
            lead: "Camera-correct scene files.",
            text: "A scene saved in a top or bottom view reloads with the exact camera it was saved in.",
          },
        ],
      },
      {
        h: "A workspace that fits the job",
        items: [
          {
            lead: "Three new desks:",
            text: "Technical (compact viewport, dominant node canvas with the spreadsheet beneath), LookDev (viewport-dominant with the texture viewer tabbed beside properties), and UV / Texturing. Six presets total, and saved desks keep working.",
          },
          {
            lead: "A scene Tree panel:",
            text: "the whole scene as a searchable, collapsible outline with context-colored chips; double-click to reveal or dive. It opens fully expanded, with expand-all and collapse-all buttons.",
          },
          {
            lead: "Maximize any panel,",
            text: "with backtick maximizing the panel under the cursor and Esc restoring. Closed core panels reopen from the Desks menu.",
          },
          {
            lead: "Display preferences",
            text: "for default wireframe weight, background and turntable speed, plus a turntable speed submenu in the Display menu.",
          },
          {
            lead: "A palette you can navigate:",
            text: "fourteen curated node categories replace the six coarse ones, with each node's icon in the palette, the Add menu and the list view. Every dialog drags and resizes; notes resize live and carry a text-size setting.",
          },
        ],
      },
      {
        h: "Under the hood",
        items: [
          {
            lead: "One shared attribute query",
            text: "serves the pane, the pickers and the visualization: values page across the wasm boundary a window at a time, and cooked geometry still never crosses into JavaScript.",
          },
          {
            lead: "The keystone lands.",
            text: "The attribute system is the foundation for curves, wrangling and the expression engine that follows in 0.8.1.",
          },
        ],
      },
      {
        h: "Breaking changes",
        items: [
          {
            lead: "None that block an upgrade.",
            text: ".slxy stays at schema version 1; the nodes whose shape changed (bounds, import_ply, geo_export, note) carry per-node versions and load older scenes unchanged.",
          },
          {
            lead: "Three web keyboard reassignments:",
            text: "F over the viewport is now Front (fit moved to Z), O over a 3D pane is now Orthographic (it keeps its overlap meaning in UV panes), and B toggles bypass only over the node canvas.",
          },
        ],
      },
    ],
    meta: [
      ["Registry", "58 to 74"],
      ["Schema", "stays 1"],
      ["Tests", "966 Rust, 204 web"],
      ["Payload", "1.51 MB gzip"],
    ],
  },
  {
    v: "v0.7.2",
    code: "Polish",
    date: "21 July 2026",
    status: "live",
    statusLabel: "Released",
    summary:
      "The first point release on the road to 1.0: consistency and the cheap wins, plus four new procedural texture generators that served as the best proof-by-repetition of the zero-frontend-change contract.",
    groups: [
      {
        h: "Correctness",
        items: [
          {
            lead: "Ambient occlusion actually composites.",
            text: "The occlusion-map path had four separate drop paths, one more than the milestone draft found: the compositor also bailed whenever the metallic-roughness map arrived as a file path rather than decoded bytes. The fix decodes both from bytes or a path, builds a white base when the MR map is absent so an AO-only material keeps its AO, and resamples on a size mismatch.",
          },
          {
            lead: "glTF occlusion strength is honored.",
            text: "The occlusion texture's strength now reaches the material uniform, which had been hardcoded to 1.0.",
          },
          {
            lead: "The validate node can never emit a false missing-UV error,",
            text: "behind an opt-in flag.",
          },
        ],
      },
      {
        h: "Four procedural texture generators",
        items: [
          {
            lead: "voronoi, gradient, checker and brick",
            text: "join the texture context. The gradient is differentiated from the existing ramp by a movable centre plus angular and diamond falloffs.",
          },
          {
            lead: "The zero-frontend-change contract held:",
            text: "palette, parameter panel and typed handles needed no change. Only the glyph-quality gate did, which required four pieces of dedicated node art.",
          },
        ],
      },
      {
        h: "Polish",
        items: [
          {
            lead: "The radial menu's icons became theme-aware.",
            text: "A pre-existing defect, not from this milestone: the active icon was hardcoded to dark ink and idle icons to near-white, so on the light theme's terracotta accent the active icon sat at roughly 3:1 contrast and idle icons washed out on cream.",
          },
          {
            lead: "Image nodes stopped reporting bogus geometry.",
            text: "They rendered a 0 pts / 0 tris / 0 mesh line beside their dimensions, because the engine emits zero rather than nothing for image nodes.",
          },
          {
            lead: "Model preview moved to the worker.",
            text: "Opening a preview had coupled parse, GPU upload and render on the main wasm instance; it split into a worker parse plus a host handoff, with a stale-blob guard so a superseded preview never displays.",
          },
        ],
      },
    ],
    meta: [
      ["Registry", "58 to 62"],
      ["Tests", "128 web"],
      ["Payload", "1.49 MB gzip"],
    ],
  },
  {
    v: "v0.7.1",
    code: "Typed contexts, cameras, and one design language",
    date: "July 2026",
    status: "live",
    statusLabel: "Released",
    summary:
      "The beta grows up. The node graph gains typed contexts, dedicated material and texture networks beside the geometry graph, taking the catalogue from 33 to 58 node types, every one carrying real documentation in the app and in the Node Reference. Cameras become first-class nodes you can look through, scenes export to standard formats, and a single palette drives the desktop app, the web app and the terminal UI.",
    groups: [
      {
        h: "Typed node contexts",
        items: [
          {
            lead: "Four network kinds:",
            text: "Obj (the scene root), Geo, Mat and Tex. Containers declare what they open, so no container type is special-cased by its id.",
          },
          {
            lead: "Texture networks:",
            text: "14 image nodes cooking on the CPU at a working resolution, in a new solarxy-imaging crate. The workspace reaches 12 members.",
          },
          {
            lead: "Material networks:",
            text: "principled, matcap, toon, unlit and mix_material surfaces, plus tex_ref to pull an image out of a texture network, with per-slot mesh targeting on the geo-side material node.",
          },
          {
            lead: "Cross-context references",
            text: "travel by path, not by wire, with cycle refusal at set time.",
          },
          {
            lead: "A selection outline",
            text: "drawn as a jump-flood rim, replacing the legacy tint.",
          },
        ],
      },
      {
        h: "Cameras, export, and the front door",
        items: [
          {
            lead: "Camera nodes,",
            text: "perspective or orthographic, with focal length, sensor width and clip planes; any pane can look through one, locked or free.",
          },
          { lead: "Turntable export", text: "to WebM, MP4 or a PNG sequence." },
          {
            lead: "File export:",
            text: "geo_export writes OBJ, STL, PLY or GLB from any point in a chain; image_export writes PNG or JPEG. Geometry only at this stage; materials followed in 0.8.0.",
          },
          {
            lead: "An assets pane",
            text: "with image and model previews, and a landing page at solarxy.koljam.com.",
          },
        ],
      },
      {
        h: "Documentation, in the app and out of it",
        items: [
          {
            lead: "Every node, port and parameter documented:",
            text: "58 node overviews written against the actual cook code, plus all 342 parameters and 90 ports. The Node Reference regenerates from the same registry, so the two cannot disagree.",
          },
          {
            lead: "Honest about limits.",
            text: "Controls that do not do anything yet say so in their descriptions rather than pretending.",
          },
          { lead: "A first-run tour", text: "of seven short steps, skippable and replayable." },
        ],
      },
      {
        h: "One design language",
        items: [
          {
            lead: "One palette, three shells.",
            text: "Interface colors are defined once, in Rust, and drive the desktop GUI, the web app through generated CSS, and the analyze TUI. A drift test fails the build if they disagree.",
          },
          {
            lead: "The warm light theme became the light theme",
            text: "on desktop and web alike, with existing preference files migrating automatically.",
          },
          {
            lead: "Theme fixes across the app:",
            text: "the console log was unreadable on light themes, desktop widget hover states were invisible, the radial menu ignored the theme entirely, and the review change category was green on desktop but red on the web.",
          },
        ],
      },
    ],
    meta: [
      ["Registry", "33 to 58"],
      ["Schema", "frozen at 1"],
      ["Tests", "764 Rust, 128 web"],
      ["Payload", "1.40 MB gzip"],
    ],
  },
];

export interface ReleasePlanEntry {
  v: string;
  code: string;
  kind?: "next" | "milestone";
  theme: string;
  items: string[];
}

/* The four remaining rungs to 1.0. The full thirteen-release program lives in
 * PROGRAM below; this is deliberately the same data at a shorter zoom. 1.0 is
 * a maturity bar, not a feature bar: stable, documented, embeddable,
 * publishable, explicitly not complete. */
export const RELEASE_PLAN: ReleasePlanEntry[] = [
  {
    v: "v0.9.0",
    code: "Path-traced rendering",
    theme: "A physically based GPU path tracer, and the CLI as a render surface",
    items: [
      "A compute path tracer on core WebGPU (12.5): global illumination, soft area-light shadows, optical depth of field, unbounded light count.",
      "A render command for the CLI (17.3), making it the first native node-engine host: .slxy in, PNG or EXR with AOVs out.",
      "The render window finished: the picture refines while it renders rather than appearing at the end, the albedo, normal and depth passes are selectable and viewable in the browser, and the image pans and zooms while it converges.",
      "Controls to tune a render for your own scene: an exact sample count beside the presets, an indirect clamp, a seed, denoiser strength and a threshold, and named output sizes.",
      "Rendering with a transparent background, so a still arrives as an element that can be composited over something else rather than as a finished picture.",
      "Lights can be moved by grabbing them in the viewport, which is the first thing there that is not geometry.",
      "Priya joins the personas: technical director and pipeline engineer, with two new journeys.",
      "The last release whose authoring surface is web-only.",
    ],
  },
  {
    v: "v0.9.5",
    code: "Desktop node canvas",
    kind: "next",
    theme: "Node editing on the desktop; the desktop wiring closes",
    items: [
      "The egui node canvas, palette, parameter panel, transform gizmo and undo.",
      "One core, two shells becomes literally true rather than aspirational.",
      "Sized: the node-canvas library spike inside 0.8.2 has run and found a native substrate that fits, so this release builds the canvas rather than the ground under it.",
    ],
  },
  {
    v: "v0.10.0",
    code: "Hard-surface modeling",
    theme: "The flagship modeling asks, and the runtime's first consumers",
    items: [
      "Boolean / CSG (5.3), after a Rust CSG crate evaluation spike.",
      "The extrude / sweep / revolve / loft family (5.4) and FBX / USD import (15.2).",
      "The runtime's first structural consumers: the event system (7.2) and keyframe channels plus timeline UI (8.2).",
      "The first release designed for two surfaces from the start.",
    ],
  },
  {
    v: "v1.0.0",
    code: "The Horizon",
    kind: "milestone",
    theme: "Trustworthy, not complete",
    items: [
      "A stable solarxy-core on crates.io, a semver commitment, an API freeze, and a .slxy forward-migration guarantee.",
      "The framework embed API (16.2), end-to-end browser coverage (18.7) and wasm threads via cross-origin isolation (18.5).",
      "Deliberately thin at card level. 1.0 means the engine is stable and embeddable; capability ships as 1.x.",
      "The desktop wiring is no longer here: it split across 0.8.2 and 0.9.5.",
    ],
  },
];

export interface ProgramEntry {
  v: string;
  code: string;
  kind: "shipped" | "next" | "planned" | "milestone";
  era: "pre" | "mark" | "post";
  theme: string;
  cards: string[];
  who: string;
  note?: string;
}

/* The full milestone program: thirteen numbered releases, five of them to 1.0.
 * Everything past 1.5 is Backlog with a trigger rather than an invented
 * number. Release and disposition are derived from this array and
 * BACKLOG_WAVES below, never stored on a card, so the two cannot disagree. */
export const PROGRAM: ProgramEntry[] = [
  {
    v: "0.7.2",
    code: "Polish",
    kind: "shipped",
    era: "pre",
    theme: "Consistency and the first cheap wins",
    cards: ["11.2"],
    who: "Sam",
  },
  {
    v: "0.8.0",
    code: "First modeling wave",
    kind: "shipped",
    era: "pre",
    theme: "The points, curves and attributes foundation",
    cards: ["5.1", "5.2", "15.1", "18.4"],
    who: "Deniz, Mara",
  },
  {
    v: "0.8.1",
    code: "Expressions, Runtime, Publishing",
    kind: "shipped",
    era: "pre",
    theme: "Expressions, the runtime, and the first delivery format",
    cards: ["6.1", "6.2", "7.1", "8.1", "10.4", "16.1", "18.2"],
    who: "Deniz, Sam",
  },
  {
    v: "0.8.2",
    code: "Rendering foundations",
    kind: "shipped",
    era: "pre",
    theme: "Five renderer gaps, the shared host, the desktop's first engine surface, and the terminal workspace",
    cards: ["10.3", "11.4", "18.1"],
    who: "Ingrid, Mara",
    note: "the engine-surface half of 18.1 only, not the canvas",
  },
  {
    v: "0.9.0",
    code: "Path-traced rendering",
    kind: "shipped",
    era: "pre",
    theme: "A GPU path tracer, and the CLI as a render surface",
    cards: ["12.5", "17.3"],
    who: "Ingrid, Priya",
  },
  {
    v: "0.9.5",
    code: "Desktop node canvas",
    kind: "next",
    era: "pre",
    theme: "Node editing on the desktop; card 18.1 closes",
    cards: ["18.1"],
    who: "Mara, Deniz",
    note: "the canvas half of 18.1",
  },
  {
    v: "0.10.0",
    code: "Hard-surface modeling",
    kind: "planned",
    era: "pre",
    theme: "The flagship modeling asks, and the runtime's first consumers",
    cards: ["5.3", "5.4", "7.2", "8.2", "15.2"],
    who: "Deniz, Mara",
  },
  {
    v: "1.0.0",
    code: "The Horizon",
    kind: "milestone",
    era: "mark",
    theme: "Trustworthy, not complete: stable, documented, embeddable, publishable",
    cards: ["16.2", "18.5", "18.7"],
    who: "Everyone",
  },
  {
    v: "1.1.0",
    code: "Material and render depth",
    kind: "planned",
    era: "post",
    theme: "Depth on what everyone already touches, and the committed render work's remainder",
    cards: ["10.1", "10.2", "12.1", "12.4", "12.6"],
    who: "Ingrid, Mara",
  },
  {
    v: "1.2.0",
    code: "Modeling depth",
    kind: "planned",
    era: "post",
    theme: "Deformers, topology, unwrapping, selection and measurement",
    cards: ["5.5", "5.6", "5.7", "5.8", "5.9"],
    who: "Deniz, Mara",
  },
  {
    v: "1.3.0",
    code: "Animation",
    kind: "planned",
    era: "post",
    theme: "Animation depth on the clock and keyframes that already shipped",
    cards: ["8.3", "8.4", "8.5"],
    who: "Ingrid, Deniz",
  },
  {
    v: "1.4.0",
    code: "Interactivity",
    kind: "planned",
    era: "post",
    theme: "A scene that responds, without anyone writing code",
    cards: ["7.3", "7.4", "16.5"],
    who: "Deniz, Sam",
  },
  {
    v: "1.5.0",
    code: "Platform",
    kind: "planned",
    era: "post",
    theme: "Solarxy becomes something you build with, not only in",
    cards: ["16.3", "16.4", "17.1", "17.2"],
    who: "Priya, Deniz",
  },
];

export interface Disposition {
  key: "shipped" | "scheduled" | "backlog" | "deferred" | "wont";
  label: string;
  n: number;
  blurb: string;
}

export const DISPOSITIONS: Disposition[] = [
  {
    key: "shipped",
    label: "Shipped",
    n: 16,
    blurb: "Already released, most recently the 0.9.0 path tracer and the render command that made the terminal a first-class render surface.",
  },
  {
    key: "scheduled",
    label: "Scheduled",
    n: 29,
    blurb: "Assigned to a named release between v0.9.0 and v1.5.0.",
  },
  {
    key: "backlog",
    label: "Backlog",
    n: 20,
    blurb: "Wanted, unscheduled, each naming the trigger that would schedule it.",
  },
  {
    key: "deferred",
    label: "Deferred",
    n: 6,
    blurb: "The deferrals stand: CAD and B-rep, cloth, a WebGL2 fallback, and the three XR cards.",
  },
  {
    key: "wont",
    label: "Won't-do",
    n: 1,
    blurb: "Proposed for removal: camera-based tracking, whose successor is the plugin system.",
  },
];

export interface BacklogWave {
  w: string;
  cards: string[];
  trig: string;
}

export const BACKLOG_WAVES: BacklogWave[] = [
  {
    w: "Simulation",
    cards: ["9.1", "9.2", "9.3"],
    trig: "The runtime has two shipped structural consumers. 7.2 and 8.2 both land in v0.10.0, so this fires there.",
  },
  {
    w: "Shader and post graphs",
    cards: ["6.3", "6.4", "12.2"],
    trig: "A permutation-management plan exists that does not break the zero-pipeline-permutation stance.",
  },
  {
    w: "Texture breadth",
    cards: ["11.1", "11.3", "11.5", "11.6"],
    trig: "After v1.1.0's material depth establishes what the texture pipeline actually needs.",
  },
  {
    w: "Interchange",
    cards: ["15.3", "15.4", "15.5"],
    trig: "Demand-driven. Each is independently small and can ride any release with capacity.",
  },
  {
    w: "Audio",
    cards: ["13.1", "13.2", "13.3"],
    trig: "The runtime is proven by v1.4.0's interactivity work, and a real user asks.",
  },
  {
    w: "Volumetrics",
    cards: ["12.3"],
    trig: "After v1.1.0's post catalog establishes the effect infrastructure.",
  },
  { w: "SDF and volume", cards: ["5.10"], trig: "After v1.2.0's modeling depth." },
  {
    w: "Text to geometry",
    cards: ["5.12"],
    trig: "Any time. Small, independent, a good filler for a release with capacity.",
  },
  {
    w: "Wire types",
    cards: ["18.3"],
    trig: "Continuous, per addition. A standing practice rather than a milestone.",
  },
];

export const DEFERRED_IDS: string[] = ["5.11", "9.4", "18.6", "14.1", "14.2", "14.3"];
export const WONT_IDS: string[] = ["13.4"];

export interface Persona {
  idx: string;
  name: string;
  cls: string;
  role: string;
  care: string;
  jr: string[];
}

export const PERSONAS: Persona[] = [
  {
    idx: "P1",
    name: "Mara",
    cls: "p-coral",
    role: "Technical artist / asset-QA lead, Expert",
    care: "Validates vendor deliveries for a game pipeline. Cares about import robustness on messy files, the validate node and overlays, viewport picking to find the offending node, per-node cook stats, and round-trippable annotations.",
    jr: ["J2", "J6", "J8"],
  },
  {
    idx: "P2",
    name: "Deniz",
    cls: "p-lav",
    role: "Parametric designer, Expert",
    care: "Grasshopper / Houdini background, new to Solarxy. Cares about predictable typed ports, variadic merge, composable transform chains, manual cook mode on heavy graphs, copy / paste muscle memory, and eventually expressions.",
    jr: ["J1", "J5", "J7"],
  },
  {
    idx: "P3",
    name: "Ingrid",
    cls: "p-peach",
    role: "Archviz / lighting designer, Intermediate",
    care: "Comes for composition and lighting studies over imported models. Cares about the six light types, understandable shadow control, the HDRI environment, split viewports, and clean screenshots.",
    jr: ["J3"],
  },
  {
    idx: "P4",
    name: "Sam",
    cls: "p-lav",
    role: "Student / creative coder, Novice",
    care: "Browser-first; zero-install is the hook. Cares about first-session learnability, the Tab palette, per-node help, undo that always works, autosave, and errors that explain rather than punish.",
    jr: ["J4", "J7", "J9", "J16"],
  },
  {
    idx: "P5",
    name: "Priya",
    cls: "p-coral",
    role: "Technical director / pipeline engineer, Expert",
    care: "Integrates Solarxy into a studio or product pipeline. Cares about headless operation, deterministic and reproducible output, machine-readable results and meaningful exit codes, containerized runs, version-pinned scene files, and render checks that fail a build rather than a human.",
    jr: ["J11", "J12"],
  },
];

export interface Journey {
  id: string;
  who: string;
  t: string;
  b: string;
}

export const JOURNEYS: Journey[] = [
  {
    id: "J1",
    who: "Deniz",
    t: "Build a parametric asset from primitives",
    b: "Palette to place primitives, typed handles that only connect what is legal, a variadic merge with drag-reorder, precision-drag on params, the display flag to preview, subflow navigation via breadcrumbs, and save.",
  },
  {
    id: "J2",
    who: "Mara",
    t: "Import, validate, and annotate a delivery",
    b: "Drag a 100 MB glTF, watch worker progress, wire a validate node with its report port and panel, read the validation overlay, fly the camera to an issue, drop review annotations, and save with embedded assets.",
  },
  {
    id: "J3",
    who: "Ingrid",
    t: "Lighting study over an imported model",
    b: "Additive lights, the exclusive shadow caster (checking a new caster animates the old one off with a toast naming the light that lost the shadow), light helpers, the environment panel, split / quad viewports, and the screenshot modal.",
  },
  {
    id: "J4",
    who: "Sam",
    t: "First session",
    b: "Empty states that point at Tab, palette discovery, a param-docs popover, and an accidental tab close met by an autosave recovery prompt rather than lost work.",
  },
  {
    id: "J5",
    who: "Deniz",
    t: "Tame a heavy graph",
    b: "Switch to Manual cook mode, watch stale badges propagate, press Cook, read per-node cook stats to find the slow node, bypass suspects, and return to Auto.",
  },
  {
    id: "J6",
    who: "Mara",
    t: "Bisect a bad mesh via picking",
    b: "Click a broken object so its producing geo node selects, double-click into the subflow, walk upstream by moving the display flag, bypass suspects, and read info-popover stats (zero UVs).",
  },
  {
    id: "J7",
    who: "Deniz, Sam",
    t: "Duplicate and vary",
    b: "Select a three-node chain, duplicate it with internal wiring intact and offset, edit the copy, and paste into a different geo subflow; a root-only note is skipped with a toast.",
  },
  {
    id: "J8",
    who: "Mara",
    t: "UV cross-check",
    b: "Bind a pane to the UV view of a selected import node, trigger overlap detection, watch the async GPU readback pending indicator, read the overlap percentage and highlight, and add a UV annotation.",
  },
  {
    id: "J9",
    who: "Sam, Deniz",
    t: "Publish a scene",
    b: "Author it, use the File menu's Export web bundle, drop the folder on any static host, and send a link that plays for someone with no install and no account.",
  },
  {
    id: "J10",
    who: "Deniz",
    t: "Drive one node from another",
    b: 'Hover a numeric param, press the = affordance, type ch("../box1/size") * 2, watch the value resolve beneath the field, rename box1, and watch the expression follow rather than break.',
  },
  {
    id: "J11",
    who: "Priya",
    t: "Render a scene from the terminal",
    b: "Save a scene on the web, render it headless with a fixed seed, get an EXR with albedo and normal AOVs, and confirm it matches what the browser produced for the same file.",
  },
  {
    id: "J12",
    who: "Priya",
    t: "Gate a build on a render check",
    b: "In a container, render twice with the same seed and confirm bit-identical output; corrupt the scene's camera reference and confirm a meaningful exit code with a diagnostic on stderr and nothing on stdout.",
  },
  {
    id: "J13",
    who: "Mara",
    t: "Open a web-authored scene on the desktop",
    b: "A colleague authors and saves; Mara opens the .slxy natively, sees the same geometry under the same lighting, runs validation, drops annotations, and saves. She never opens a browser.",
  },
  {
    id: "J14",
    who: "Ingrid",
    t: "Take a shot from lookdev to finished frame",
    b: "Build the look against a converging traced preview, set aperture and focus on a foreground object, render a still with real depth of field, and grade it with a LUT that travels with the camera.",
  },
  {
    id: "J15",
    who: "Ingrid, Deniz",
    t: "Animate a scene",
    b: "Set keyframes on parameters, animate a camera along a curve, scrub the timeline, and export a sequence.",
  },
  {
    id: "J16",
    who: "Deniz, Sam",
    t: "Make a scene respond",
    b: "Wire a pointer event to a behaviour, route a trigger through state, publish it, and have it react to a visitor who has never opened Solarxy.",
  },
];

export const COVERAGE_RELEASES: string[] = [
  "0.9.5",
  "0.10.0",
  "1.0.0",
  "1.1.0",
  "1.2.0",
  "1.3.0",
  "1.4.0",
  "1.5.0",
];

export const COVERAGE: Record<string, number[]> = {
  J1: [1, 1, 0, 0, 1, 0, 0, 1],
  J2: [0, 1, 0, 0, 0, 0, 0, 0],
  J3: [0, 0, 0, 1, 0, 0, 0, 0],
  J4: [0, 0, 1, 0, 0, 0, 0, 0],
  J5: [1, 0, 1, 0, 0, 0, 0, 1],
  J6: [1, 0, 0, 0, 1, 0, 0, 0],
  J7: [1, 0, 0, 0, 0, 0, 0, 1],
  J8: [0, 0, 0, 0, 1, 0, 0, 0],
  J9: [0, 0, 1, 0, 0, 0, 1, 0],
  J10: [1, 0, 0, 0, 0, 1, 0, 1],
  J11: [0, 0, 0, 0, 0, 0, 0, 0],
  J12: [0, 0, 1, 0, 0, 0, 0, 0],
  J13: [1, 0, 0, 0, 0, 0, 0, 0],
  J14: [0, 0, 0, 1, 0, 0, 0, 0],
  J15: [0, 1, 0, 0, 0, 1, 0, 0],
  J16: [0, 1, 0, 0, 0, 0, 1, 0],
};

export const UX_CONTRACT: string[] = [
  "A parameter drag reaches the viewport within 1 to 2 frames.",
  "The viewport never blanks on a failed or empty cook: the last good output is kept.",
  "A superseded async import never displays.",
  "The UI thread never blocks longer than about a frame.",
  "Manual cook mode never lies about stale state.",
];

export interface Theme {
  key: string;
  num: string;
  title: string;
  blurb: string;
}

export const THEMES: Theme[] = [
  {
    key: "modeling",
    num: "5",
    title: "Procedural modeling (geometry)",
    blurb: "The largest single gap and the most identity-aligned place to invest.",
  },
  {
    key: "vp",
    num: "6",
    title: "Visual programming, expressions & shader graph",
    blurb: "Logic and shading as node graphs that compile to code.",
  },
  {
    key: "runtime",
    num: "7",
    title: "Interactivity & runtime",
    blurb: "The single biggest leap from viewer toward creative engine.",
  },
  {
    key: "anim",
    num: "8",
    title: "Animation & timeline",
    blurb: "The most tractable consumer of the runtime; closest to shipped turntable work.",
  },
  {
    key: "sim",
    num: "9",
    title: "Simulation & dynamics",
    blurb: "A large capability area with one feasibility advantage: Rapier is Rust.",
  },
  {
    key: "materials",
    num: "10",
    title: "Materials & shading",
    blurb: "What comes after the committed render expansion closes the immediate gap.",
  },
  {
    key: "textures",
    num: "11",
    title: "Textures & imaging",
    blurb: "A clean CPU set of 14 nodes; growth here is mostly incremental.",
  },
  {
    key: "render",
    num: "12",
    title: "Rendering & post-processing",
    blurb: "The remainder after the committed render expansion: a far larger post catalog and beyond.",
  },
  {
    key: "audio",
    num: "13",
    title: "Audio & media",
    blurb: "Scoped as a WebAudio bridge, not a Rust DSP engine.",
  },
  {
    key: "xr",
    num: "14",
    title: "XR / AR / VR",
    blurb: "Gated on WebGPU-plus-WebXR interop maturity.",
  },
  {
    key: "io",
    num: "15",
    title: "I/O, assets & formats",
    blurb: "Mostly incremental additions addressing the most-felt practical gaps.",
  },
  {
    key: "scene",
    num: "16",
    title: "Scene, document, workflow & deployment",
    blurb: "Holds the most defining creative-engine capability Solarxy lacks: shippable scenes.",
  },
  {
    key: "ext",
    num: "17",
    title: "Extensibility & ecosystem",
    blurb: "Turns Solarxy from an app into a platform.",
  },
  {
    key: "internal",
    num: "18",
    title: "Solarxy-internal strategic gaps",
    blurb: "Not parity features, but the leverage points that make the rest cheaper.",
  },
];

export interface Enabler {
  g: string;
  id: string;
  name: string;
  ref: string;
  p: string;
  grades: string;
}

export const ENABLERS: Enabler[] = [
  {
    g: "A",
    id: "5.1",
    name: "Points, curves & attributes",
    ref: "Card 5.1, theme 5",
    p: "Extend GeometrySet beyond triangle meshes to carry point clouds, poly-lines / curves and a general typed per-element attribute system, with point / line draw in the renderer. The keystone that unblocks the entire modeling wave.",
    grades: "High, L, Adaptable",
  },
  {
    g: "B",
    id: "6.1",
    name: "Expression engine",
    ref: "Card 6.1, theme 6",
    p: "Turn on the reserved expression seam: a sandboxed grammar so a param can compute, reference another node, or read scene time. Multiplies the value of every existing node.",
    grades: "High, L, Structural",
  },
  {
    g: "C",
    id: "7.1",
    name: "Runtime / play model",
    ref: "Card 7.1, theme 7",
    p: "A scene clock, a per-frame tick, play / pause / reset, and event dispatch, so the document evolves over time. The shared prerequisite for interactivity, animation, and simulation.",
    grades: "High, L-XL, Structural",
  },
];

export interface ShortlistItem {
  b: string;
  s: string;
}

export const SHORTLIST: ShortlistItem[] = [
  {
    b: "Points / curves / attributes core",
    s: "plus scatter, copy-to-points and curve primitives. Turns Solarxy from a primitive-assembler into a procedural tool.",
  },
  {
    b: "Boolean / CSG",
    s: "the flagship modeling ask, with a recorded pre-task to evaluate Rust CSG crates first.",
  },
  {
    b: "Expression engine",
    s: "plus attribute wrangle. Multiplies the value of every existing node.",
  },
  {
    b: "Standalone / embeddable web export",
    s: "the defining creative-engine capability Solarxy most conspicuously lacked.",
  },
  {
    b: "Material export from geo_export",
    s: "plus FBX / USD import. Closes the most-felt I/O gaps.",
  },
];

export const ORDERING: string[] = [
  "The points / curves / attributes core, plus the cheap modeling nodes it unblocks.",
  "Expressions, plus attribute wrangle and parametric params.",
  "Boolean / CSG and the heavier operators (extrude / sweep / loft).",
  "The runtime and play model, then animation and timeline on top of it.",
  "Interactivity (actor / event) and simulation, the largest structural leaps.",
  "Breadth themes: audio, XR, expanded post, deployment / export, plugins.",
];

export interface Card {
  id: string;
  t: string;
  title: string;
  impact: string;
  effort: string;
  fit: string;
  tier: "Foundational" | "Near" | "Mid" | "Long" | "Aspirational";
  en?: string;
  ships?: string;
  planned?: string;
  split?: string[];
  what: string;
  why: string;
  dep: string;
  risk: string;
}

export const CARDS: Card[] = [
  {
    id: "5.1",
    t: "modeling",
    title: "Points, curves & lines + attributes",
    impact: "High",
    effort: "L",
    fit: "Adaptable",
    tier: "Foundational",
    en: "A",
    ships: "0.8.0",
    what: "Extend GeometrySet beyond triangle meshes to carry point clouds, poly-lines / curves and a general typed per-element attribute system, with point / line draw in the renderer.",
    why: "The keystone; nearly every interesting modeling node (scatter, curves, wrangle, measure) is blocked on it.",
    dep: "Depends on nothing. Unblocks 5.2, 5.4, 5.8, 5.9, 6.2 and renderer instancing.",
    risk: "Keep attributes internal to GeometrySet, not new wire types, to avoid coercion-matrix churn.",
  },
  {
    id: "5.2",
    t: "modeling",
    title: "Scatter & copy-to-points",
    impact: "High",
    effort: "M",
    fit: "Native",
    tier: "Near",
    ships: "0.8.0",
    what: "Area-weighted, seeded point sampling on a surface, paired with instancing a source geometry onto each point with per-point variation.",
    why: "The procedural multiplier; the most-requested capability after boolean for parametric and archviz work.",
    dep: "Depends on 5.1. Pairs with instancer-at-scale.",
    risk: "Instance-count ceilings and GPU-instancing vs CPU-merge policy; reuse the array triangle-ceiling precedent.",
  },
  {
    id: "5.3",
    t: "modeling",
    title: "Boolean / CSG",
    impact: "High",
    effort: "L",
    fit: "Adaptable",
    tier: "Near",
    what: "A boolean node (union, subtract, intersect) on two mesh inputs.",
    why: "The flagship modeling ask; hard-surface modeling is impractical without it.",
    dep: "Evaluate Rust CSG crates in a spike first; benefits from 5.1 for seam preservation.",
    risk: "Robustness on coplanar faces and degenerate inputs is the classic CSG pitfall. Budget a spike (XL if own implementation).",
  },
  {
    id: "5.4",
    t: "modeling",
    title: "Extrude, sweep, revolve, loft, bevel",
    impact: "High",
    effort: "L",
    fit: "Adaptable",
    tier: "Mid",
    what: "Turn profiles and faces into solids: extrude with inset, revolve around an axis, sweep / loft along paths, and bevel.",
    why: "The core of constructive, non-primitive modeling; pairs with curves to make Solarxy a real modeler.",
    dep: "Curve-based sweep / loft depend on 5.1; extrude on faces does not.",
    risk: "Robust inset and self-intersection handling; bevel topology is fiddly.",
  },
  {
    id: "5.5",
    t: "modeling",
    title: "Deformers: bend, twist, taper, lattice, noise",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Non-destructive spatial deformers driven by params or attributes, including a lattice cage and attribute / texture-driven displacement.",
    why: "Cheap, high-visual-value nodes that make procedural graphs feel organic; noise-displace is a staple.",
    dep: "Texture-driven displace benefits from 5.1 attributes; math deformers do not.",
    risk: "Normal recomputation after displacement (reuse compute_normals).",
  },
  {
    id: "5.6",
    t: "modeling",
    title: "Topology: remesh, decimate, Catmull-Clark, repair",
    impact: "Medium",
    effort: "M-L",
    fit: "Native",
    tier: "Mid",
    what: "Uniform retopo, triangle reduction, true Catmull-Clark subdivision (the promised successor to linear), and mesh-repair tied to the validator.",
    why: "Import cleanup and LOD; mesh-repair is a natural bridge to Solarxy's validation identity.",
    dep: "Mesh-repair can consume existing validation issues (NonManifoldEdge, DegenerateTriangles).",
    risk: "Decimation / remesh quality varies by algorithm; likely a crate evaluation.",
  },
  {
    id: "5.7",
    t: "modeling",
    title: "UV unwrap (conformal / angle-based)",
    impact: "Medium",
    effort: "L",
    fit: "Native",
    tier: "Mid",
    what: "Real seam-based conformal or angle-based unwrap plus a UV layout / pack step, beyond the current planar / box / cylindrical / spherical projection.",
    why: "Imports often lack UVs; texel density needs them; a real unwrap closes a QA-adjacent gap.",
    dep: "Feeds texture baking (11.5) and texel-density QA.",
    risk: "Unwrap solvers are nontrivial; consider a crate.",
  },
  {
    id: "5.8",
    t: "modeling",
    title: "Groups & selection system",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "A reusable named group / selection so operators act on a subset of primitives (bbox, normal, attribute expression, or manual pick).",
    why: "The multiplier that lets one operator become many; foundational for expressive graphs.",
    dep: "Depends on 5.1; strongly amplified by 6.1 expressions.",
    risk: "Design the group grammar without over-building; start at parity with delete's bbox / normal filtering.",
  },
  {
    id: "5.9",
    t: "modeling",
    title: "Measure, analysis, ray & project",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Nodes that compute geometry attributes (curvature, area, edge length) and ray / project points onto a surface, plus adjacency analysis.",
    why: "Analysis attributes feed displacement, scatter density and coloring; adjacent to the QA identity.",
    dep: "Depends on 5.1 to store results; reuses the core Moller-Trumbore raycaster. Feeds 5.5 and 5.2.",
    risk: "Low.",
  },
  {
    id: "5.10",
    t: "modeling",
    title: "SDF & volume modeling",
    impact: "Medium",
    effort: "L-XL",
    fit: "Adaptable",
    tier: "Long",
    what: "Signed-distance-field modeling: SDF primitives, smooth boolean / union, offset, and marching-cubes triangulation back to mesh.",
    why: "Smooth blends and robust booleans that mesh CSG struggles with; the gateway to raymarched materials.",
    dep: "An alternative to 5.3; unblocks raymarched materials.",
    risk: "Meshing quality and performance; a second geometry representation to maintain.",
  },
  {
    id: "5.11",
    t: "modeling",
    title: "CAD / B-rep (STEP) import & modeling",
    impact: "Medium",
    effort: "XL",
    fit: "Research",
    tier: "Aspirational",
    what: "Boundary-representation CAD via an OpenCascade-class kernel: STEP import, exact fillets, precise booleans, tessellate-to-mesh.",
    why: "Precision engineering and product workflows; a distinct market from the archviz / game audience.",
    dep: "Best as an optional plugin once 5.1 and the plugin system exist.",
    risk: "Large dependency, licensing, and wasm-size cost; do not fold into core.",
  },
  {
    id: "5.12",
    t: "modeling",
    title: "Text-to-geometry",
    impact: "Low-Med",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "A text node that generates 3D geometry from a string plus a font (extruded or flat).",
    why: "Common in motion / archviz / product scenes; low complexity once curves and extrude exist.",
    dep: "Depends on curves (5.1) and extrude (5.4).",
    risk: "Font parsing / licensing; use an existing Rust font-outline crate.",
  },
  {
    id: "6.1",
    t: "vp",
    title: "Expression language for parameters",
    impact: "High",
    effort: "L",
    fit: "Structural",
    tier: "Foundational",
    en: "B",
    ships: "0.8.1",
    what: "Turn on the reserved expression seam: a small, sandboxed grammar so a param can be =2*$pi*r, reference another node's output, or read scene time.",
    why: "Multiplies the value of every existing node; the precondition for wrangle, driven switches, and parametric relationships.",
    dep: "Storage and UI seam already exist. Unblocks 6.2, 5.8 and driven switch / copy.",
    risk: "Sandboxing and cycle detection (the engine already refuses reference cycles); determinism for goldens; keep the grammar small.",
  },
  {
    id: "6.2",
    t: "vp",
    title: "Attribute wrangle",
    impact: "High",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    ships: "0.8.1",
    what: "A node that runs a small expression per point / primitive to read and write attributes (position, color, custom lanes).",
    why: "One wrangle replaces a dozen special-purpose nodes; the power-user centerpiece.",
    dep: "Depends on 5.1 and 6.1.",
    risk: "Per-element evaluation performance; may need a compiled / vectorized fast path for large meshes.",
  },
  {
    id: "6.3",
    t: "vp",
    title: "Visual shader graph compiling to WGSL",
    impact: "High",
    effort: "XL",
    fit: "Structural",
    tier: "Long",
    what: "A Mat-context node graph composing shading (noise, ramp, fresnel, mix, texture sample, math) that compiles to a WGSL fragment contribution.",
    why: "Custom looks without forking the renderer; the ceiling on visual expression, and the home for raymarched materials.",
    dep: "Independent of the runtime; heavy renderer work.",
    risk: "In tension with the deliberate zero-pipeline-permutations stance. A fixed node set compiling into one uber-shader with uniform branching may be the pragmatic middle path.",
  },
  {
    id: "6.4",
    t: "vp",
    title: "GPU procedural texture graph",
    impact: "Medium",
    effort: "L",
    fit: "Structural",
    tier: "Long",
    what: "Let the texture context compute generators / filters on the GPU (compute shaders) rather than only CPU, enabling higher-res, animated and feedback textures.",
    why: "Real-time and high-resolution procedural textures; audio / video reactivity later.",
    dep: "Shares infrastructure with 6.3; pairs with video textures.",
    risk: "Sync-to-async texture cook changes the cook contract; keep the CPU path as the default.",
  },
  {
    id: "7.1",
    t: "runtime",
    title: "Runtime / play model",
    impact: "High",
    effort: "L-XL",
    fit: "Structural",
    tier: "Foundational",
    en: "C",
    ships: "0.8.1",
    what: "A scene clock, a per-frame tick, a play / pause / reset state, and an event-dispatch path, so the document can do something over time rather than only cook to a static scene.",
    why: "The shared prerequisite for interactivity, animation playback and simulation. Nothing in themes 7 to 9 is possible without it.",
    dep: "Depends on nothing, but is the gate for the whole runtime half of the roadmap. Unblocks 7.2, 7.3, all of theme 8, and simulation stepping.",
    risk: "Reconciling a live tick with pull-based cooking and undo; determinism for goldens; keeping desktop and web in sync. Design spike warranted.",
  },
  {
    id: "7.2",
    t: "runtime",
    title: "Event system (pointer, raycast, keyboard, scroll)",
    impact: "High",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Nodes that turn browser / device input into graph signals: object click / hover via GPU picking, pointer / keyboard, scroll triggers, and a raycast toolkit.",
    why: "The input half of interactivity; reuses the picking infrastructure Solarxy already has.",
    dep: "Depends on 7.1; feeds 7.3.",
    risk: "Boundary design: input events must cross JS to Rust cleanly without breaking mirror-and-command.",
  },
  {
    id: "7.3",
    t: "runtime",
    title: "No-code behavior / actor graph",
    impact: "High",
    effort: "XL",
    fit: "Structural",
    tier: "Long",
    what: "A behavior context where nodes express per-object logic: set position / rotation / material on a trigger, tween, toggle visibility, run a state machine.",
    why: "What makes an engine build interactive 3D without code; the headline creative-engine capability.",
    dep: "Depends on 7.1 and 7.2.",
    risk: "The biggest single design surface in the roadmap. Prototype a minimal set (OnTick, OnClick, SetTransform, Switch) before committing to the full vocabulary.",
  },
  {
    id: "7.4",
    t: "runtime",
    title: "Trigger routing & state",
    impact: "Medium",
    effort: "M",
    fit: "Structural",
    tier: "Long",
    what: "The flow-control layer for behaviors: delay, debounce, throttle, sequence, filter, any / all, and a small state store, so behaviors compose.",
    why: "Without routing and state, behaviors are toys; with it, they are experiences.",
    dep: "Depends on 7.3.",
    risk: "Shares 7.3's risks.",
  },
  {
    id: "8.1",
    t: "anim",
    title: "Scene clock & playback",
    impact: "High",
    effort: "M",
    fit: "Adaptable",
    tier: "Near",
    ships: "0.8.1",
    what: "A transport (play / pause / scrub), a frame range, and fps, exposed in the UI, so time-dependent nodes and animations advance.",
    why: "The user-facing surface of the runtime; also the substrate for turntable and camera-path export.",
    dep: "Depends on 7.1; unblocks 8.2 to 8.5. Frame range and fps ride defaulted fields, no schema bump.",
    risk: "Shipped in 0.8.1 alongside the runtime: the clock, transport commands and events, and a transport bar.",
  },
  {
    id: "8.2",
    t: "anim",
    title: "Keyframe channels & timeline UI",
    impact: "High",
    effort: "L",
    fit: "Structural",
    tier: "Mid",
    what: "Keyframeable channels with linear / bezier tangents and a dope-sheet / curve-editor panel; set a key on any animatable param.",
    why: "The core of authored animation; the thing motion designers expect.",
    dep: "Depends on 8.1; pairs with 8.3. Dockview already hosts panes.",
    risk: "UI complexity of a curve editor; start with a dope sheet.",
  },
  {
    id: "8.3",
    t: "anim",
    title: "Param animation via channels & expressions",
    impact: "Medium",
    effort: "S",
    fit: "Native",
    tier: "Mid",
    what: "Bind a param to a channel or a time-driven expression so it animates; switch driven by =$frame becomes a flipbook.",
    why: "The glue that makes the clock and timeline actually affect the scene.",
    dep: "Depends on 6.1 and 8.2.",
    risk: "Low.",
  },
  {
    id: "8.4",
    t: "anim",
    title: "Camera paths & animated cameras",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Animate the camera node (position / target / fov) along a path or via keyframes for fly-throughs and turntable-plus.",
    why: "Extends the already-shipped turntable and still-render work into real camera animation.",
    dep: "Depends on 8.2; consumes the existing turntable / still export paths.",
    risk: "Low.",
  },
  {
    id: "8.5",
    t: "anim",
    title: "Skeletal & glTF animation playback",
    impact: "Medium",
    effort: "L",
    fit: "Structural",
    tier: "Long",
    what: "Play imported rigged / animated glTF (skinning plus animation clips) in the viewport.",
    why: "Character and asset review with animation; a real gap for anyone importing animated glTF.",
    dep: "Depends on 8.1; needs renderer skinning (joints / weights, skin matrices).",
    risk: "Skinning uniform / attribute plumbing; large-rig performance.",
  },
  {
    id: "9.1",
    t: "sim",
    title: "Rigid-body physics (Rapier, native)",
    impact: "High",
    effort: "L",
    fit: "Structural",
    tier: "Mid",
    what: "A physics context: assign rigid bodies / colliders to objects, set a world (gravity), and step the simulation for drops, stacking, and constraints.",
    why: "A marquee creative-engine feature, and unusually feasible here because Rapier is a Rust library, not a JS binding.",
    dep: "Depends on 7.1; results applied as per-frame transforms through the scene contract.",
    risk: "Determinism across platforms; wasm build / perf of Rapier; step-vs-cook reconciliation.",
  },
  {
    id: "9.2",
    t: "sim",
    title: "Particle system (GPU compute)",
    impact: "Medium",
    effort: "L",
    fit: "Structural",
    tier: "Long",
    what: "A GPU particle system: emit from geometry, integrate forces, and render as points / sprites / instances, with attribute access.",
    why: "Effects, crowds, stylized motion; pairs with 5.1 points and renderer instancing.",
    dep: "Depends on 7.1 and 5.1; overlaps 6.4 compute infra.",
    risk: "Compute-shader portability on web GPUs; readback for attribute access.",
  },
  {
    id: "9.3",
    t: "sim",
    title: "Generic solver framework (feedback loop)",
    impact: "Medium",
    effort: "M",
    fit: "Structural",
    tier: "Long",
    what: "A solver node that feeds a node's output back as next-frame input, the general substrate for iterative simulations (growth, CA, flocking).",
    why: "The extensible simulation primitive; one node enables many effects.",
    dep: "Depends on 7.1; strongly amplified by 6.2 wrangle.",
    risk: "State management within the cook model; determinism / undo semantics.",
  },
  {
    id: "9.4",
    t: "sim",
    title: "Cloth & soft body",
    impact: "Low-Med",
    effort: "XL",
    fit: "Research",
    tier: "Aspirational",
    what: "Cloth and soft-body simulation (XPBD-style), colliding with scene geometry.",
    why: "Organic secondary motion; high visual payoff for a narrow audience.",
    dep: "Depends on 7.1 and 9.1 collision.",
    risk: "High complexity for a narrow audience; defer until physics and particles prove the runtime.",
  },
  {
    id: "10.1",
    t: "materials",
    title: "Volume, points / lines & hair shading",
    impact: "Medium",
    effort: "M-L",
    fit: "Adaptable",
    tier: "Mid",
    what: "Shading models Solarxy still lacks: a volume material, a point / line material, and stylized extras.",
    why: "Point / line materials are a hard dependency of 5.1 and particles; volume is a distinct look.",
    dep: "Point / line materials depend on 5.1 and unblock particle rendering.",
    risk: "Volume rendering is a separate pass with its own cost (Structural); point / line ride the uber-shader (Adaptable).",
  },
  {
    id: "10.2",
    t: "materials",
    title: "Material library, presets & preview spheres",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "A browsable library of saved materials with live-rendered preview spheres and drag-to-apply, so materials become reusable assets.",
    why: "Reuse and discoverability; a standard expectation in DCC tools.",
    dep: "Reuses the asset pane's existing second-wgpu-surface preview pattern.",
    risk: "Deciding where library materials live: in-document vs user-global vs .slxy.",
  },
  {
    id: "10.3",
    t: "materials",
    title: "Texture maps for principled extras + true blend",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Let the principled extras be driven by texture maps (clearcoat map, transmission map), and make mix_material blend maps and shading models, not just scalars.",
    why: "Uniform-only extras are half a feature; maps are what artists use.",
    dep: "Follows the principled-surface work; additive to it.",
    risk: "Texture-slot count and bind-group growth.",
  },
  {
    id: "10.4",
    t: "materials",
    title: "Physically based area lights (LTC)",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    ships: "0.8.1",
    what: "Linearly-transformed-cosine area lights so rect-area lights shade physically, not as an approximation.",
    why: "Realistic soft lighting; a common archviz need.",
    dep: "Independent. rect_area_light already exists in the registry.",
    risk: "LUT integration and cost.",
  },
  {
    id: "11.1",
    t: "textures",
    title: "Video & webcam textures",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Video and webcam texture nodes that stream frames into the Image pipeline for animated / reactive materials.",
    why: "Motion content, live installations, AR passthrough backdrops.",
    dep: "Depends on 7.1 for per-frame update; feeds audio-reactive visuals.",
    risk: "Async decode across the boundary; frame pacing.",
  },
  {
    id: "11.2",
    t: "textures",
    title: "Expanded procedural generators",
    impact: "Medium",
    effort: "S-M",
    fit: "Native",
    tier: "Near",
    ships: "0.7.2",
    what: "More texture generators beyond constant / ramp / noise: voronoi / cellular, gradient, checker, brick / pattern, and shape / SDF fields.",
    why: "Cheap, high-value additions that make the texture context expressive without leaving CPU.",
    dep: "None; low-risk quick wins. The best proof-by-repetition of the extensibility contract.",
    risk: "None significant.",
  },
  {
    id: "11.3",
    t: "textures",
    title: "Text & canvas textures",
    impact: "Low-Med",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "A text / canvas texture node that rasterizes strings and 2D drawing into a texture (labels, UI, decals).",
    why: "Labels, decals and signage without external tools.",
    dep: "Shares a font crate with 5.12.",
    risk: "Font handling.",
  },
  {
    id: "11.4",
    t: "textures",
    title: "LUT color grading (texture & post)",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Load a 3D LUT and apply it as a texture op and / or a post grade, for filmic looks beyond lift / gamma / gain.",
    why: "Industry-standard look transfer.",
    dep: "Follows the camera-owned grade; LUTs were deferred from the beta as an asset-pipeline cost.",
    risk: "LUT format / asset plumbing (the reason it was deferred from the beta).",
  },
  {
    id: "11.5",
    t: "textures",
    title: "Texture baking",
    impact: "Medium",
    effort: "L",
    fit: "Adaptable",
    tier: "Mid",
    what: "Bake AO, normal (high-to-low), lighting, or curvature into a texture using the UVs, for optimized and stylized maps.",
    why: "Asset optimization and a bridge from Solarxy's inspection strengths to authoring.",
    dep: "Best with 5.7 quality UVs. The renderer already computes SSAO.",
    risk: "Ray-based bakes need a BVH; scope a raster-only bake first.",
  },
  {
    id: "11.6",
    t: "textures",
    title: "EXR / HDR / KTX2 + env formats",
    impact: "Low-Med",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Full EXR / HDR / KTX2 texture support and richer environment handling (equirect, cube) beyond current HDRI-for-IBL.",
    why: "HDR data textures and compressed textures (KTX2) for quality and size.",
    dep: "Reuses existing HDR / EXR decode from the IBL path.",
    risk: "KTX2 / Basis transcoding on web.",
  },
  {
    id: "12.1",
    t: "render",
    title: "Expanded post-processing catalog",
    impact: "Medium",
    effort: "M-L",
    fit: "Adaptable",
    tier: "Mid",
    what: "Effects beyond the committed set: SSR, screen-space GI, god rays, chromatic aberration, film grain, vignette, sharpen, and outline-as-a-post-effect.",
    why: "Look development and stylization; the difference between renders and renders beautifully.",
    dep: "The committed post chain has a fixed order the new effects slot into. SSR / SSGI want a GBuffer (SSAO already builds one).",
    risk: "SSR / SSGI are quality- and web-perf-sensitive (L each).",
  },
  {
    id: "12.2",
    t: "render",
    title: "Post as a node graph (POST context)",
    impact: "Medium",
    effort: "L",
    fit: "Structural",
    tier: "Long",
    what: "A POST context where the post chain is authored as nodes rather than fixed sidebar toggles, so effect order and parameters are part of the document.",
    why: "Per-shot look authoring that travels with the scene; composability.",
    dep: "Amplifies 12.1; overlaps 6.3 shader-graph infra.",
    risk: "Dynamic pass ordering vs the current fixed, optimized chain.",
  },
  {
    id: "12.3",
    t: "render",
    title: "Volumetric lighting & fog",
    impact: "Medium",
    effort: "L",
    fit: "Adaptable",
    tier: "Long",
    what: "Height / volume fog and light scattering (god rays in 3D), for atmosphere.",
    why: "Mood and depth; a common archviz / cinematic need.",
    dep: "Benefits from the committed per-light shadow maps.",
    risk: "Performance on web GPUs (Adaptable to Structural by quality target).",
  },
  {
    id: "12.4",
    t: "render",
    title: "Point-light cube-map shadows",
    impact: "Low-Med",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Omnidirectional shadows for point lights via cube maps, closing the one shadow case the committed per-light shadow work approximates.",
    why: "Correct point-light shadows; a known, scoped follow-up.",
    dep: "Follows per-light shadows; additive to the caster array.",
    risk: "6x render cost per point caster (the reason it was deferred).",
  },
  {
    id: "12.5",
    t: "render",
    title: "Path-traced / offline-quality rendering",
    impact: "Medium",
    effort: "XL",
    fit: "Adaptable",
    tier: "Near",
    planned: "0.9.0",
    what: "A physically based path tracer for reference-quality stills: global illumination, soft area-light shadows, optical depth of field, and an unbounded light count.",
    why: "Photoreal output; the top of the quality ladder. Shipped in v0.9.0.",
    dep: "Depends on v0.8.2's material model, HDR environment, instancing and shared host. Supersedes the earlier tiled raster still-render plan rather than sitting on it.",
    risk: "Regraded from Research once the feasibility spike ran: a full tracer runs on core WebGPU with no feature or limit change. Remaining risks are the wasm payload, one browser's WGSL uniformity analysis, and convergence time.",
  },
  {
    id: "12.6",
    t: "render",
    title: "IBL & environment quality",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Higher-quality IBL: better prefiltering, multiple / blended environments, and a proper environment node.",
    why: "Lighting realism; the base every material sits on.",
    dep: "Environment stays a single source of truth; a node only as a migration of the panel.",
    risk: "Keep the single-source-of-truth rule for environment.",
  },
  {
    id: "13.1",
    t: "audio",
    title: "Audio graph (synthesis & effects)",
    impact: "Medium",
    effort: "L",
    fit: "Structural",
    tier: "Long",
    what: "An audio context: sources (oscillator, sampler, file), effects (reverb, delay, filter), and routing, via a WebAudio bridge rather than a Rust DSP engine.",
    why: "Sound design for interactive and installation work; part of a full creative engine.",
    dep: "Depends on 7.1; feeds 13.3.",
    risk: "Splitting document truth between Rust and WebAudio is against the mirror model; keep audio host-owned view state.",
  },
  {
    id: "13.2",
    t: "audio",
    title: "Positional (spatial) audio",
    impact: "Low-Med",
    effort: "M",
    fit: "Adaptable",
    tier: "Long",
    what: "3D-positioned audio sources attached to objects, with a listener on the camera.",
    why: "Immersion for interactive and XR scenes.",
    dep: "Depends on 13.1; strong pairing with XR.",
    risk: "As 13.1.",
  },
  {
    id: "13.3",
    t: "audio",
    title: "Audio reactivity",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Long",
    what: "Analyse audio (FFT / waveform / amplitude) and drive geometry attributes, params, or textures from it.",
    why: "Music visualizers and reactive installations; a popular creative-web genre.",
    dep: "Depends on 7.1 and 13.1; pairs with 11.1 video.",
    risk: "Latency; boundary for streaming analyser values.",
  },
  {
    id: "13.4",
    t: "audio",
    title: "Camera-based tracking",
    impact: "Low",
    effort: "L",
    fit: "Research",
    tier: "Aspirational",
    what: "Face / hand tracking from a webcam to drive scene parameters (AR-lite, puppetry).",
    why: "Interactive installations and AR effects.",
    dep: "Depends on 7.2 input plumbing and 11.1 webcam; best as an optional plugin.",
    risk: "Heavy dependency; niche; keep out of core.",
  },
  {
    id: "14.1",
    t: "xr",
    title: "WebXR VR viewer",
    impact: "Medium",
    effort: "L-XL",
    fit: "Research",
    tier: "Aspirational",
    what: "Enter VR to view / inspect the scene in a headset, with stereo rendering and head tracking.",
    why: "Immersive review, a differentiator for the inspection audience (walk the model at scale).",
    dep: "Independent; gated on WebGPU-WebXR interop.",
    risk: "Browser support for WebGPU plus WebXR together is still maturing; spike first.",
  },
  {
    id: "14.2",
    t: "xr",
    title: "WebXR AR (passthrough) + light estimation",
    impact: "Medium",
    effort: "XL",
    fit: "Research",
    tier: "Aspirational",
    what: "Place the scene in the real world via AR passthrough, optionally lit by estimated environment light.",
    why: "Product / archviz see-it-in-your-space; a mainstream AR use case.",
    dep: "Depends on 14.1 groundwork.",
    risk: "As 14.1, plus device fragmentation.",
  },
  {
    id: "14.3",
    t: "xr",
    title: "XR input (controllers & hand tracking)",
    impact: "Low-Med",
    effort: "L",
    fit: "Research",
    tier: "Aspirational",
    what: "Controller and hand input inside XR to select / manipulate the scene.",
    why: "Interaction, not just viewing, in XR.",
    dep: "Depends on 14.1 and the 7.2 event system.",
    risk: "As 14.1.",
  },
  {
    id: "15.1",
    t: "io",
    title: "Material export from geo_export",
    impact: "High",
    effort: "M",
    fit: "Adaptable",
    tier: "Near",
    ships: "0.8.0",
    what: "Include materials (and their textures) when exporting glTF / OBJ, not just geometry.",
    why: "Geometry-only export is a hard limitation for any real asset handoff.",
    dep: "The per-feature KHR export mapping is already settled; builds on the principled material data.",
    risk: "Texture packing and coordinate / space conventions on export.",
  },
  {
    id: "15.2",
    t: "io",
    title: "FBX & USD import",
    impact: "High",
    effort: "L",
    fit: "Native",
    tier: "Mid",
    what: "Import FBX and USD / USDZ (geometry, materials, hierarchy) for interchange with the wider DCC ecosystem.",
    why: "FBX and USD are the lingua franca of production pipelines; their absence blocks whole workflows.",
    dep: "Independent.",
    risk: "FBX is proprietary / complex; USD is large. Crate quality varies; evaluate first.",
  },
  {
    id: "15.3",
    t: "io",
    title: "Compressed glTF (Draco, Meshopt)",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Decode Draco- and Meshopt-compressed glTF, which are common in delivered assets.",
    why: "A large share of real-world glTF is compressed; today's rejection is a visible wall.",
    dep: "Removes a documented rejection path in the import worker.",
    risk: "Decoder wasm size.",
  },
  {
    id: "15.4",
    t: "io",
    title: "SVG & vector import",
    impact: "Low-Med",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Import SVG paths as curves / filled geometry (logos, profiles for extrude / revolve).",
    why: "Feeds profile modeling (5.4) and 2D-to-3D workflows.",
    dep: "Depends on 5.1 curves; feeds 5.4.",
    risk: "SVG feature coverage.",
  },
  {
    id: "15.5",
    t: "io",
    title: "Broader export (USDZ, animation, scene)",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Export USDZ (AR Quick Look), animated glTF (once animation exists), and richer scene export beyond single-geometry taps.",
    why: "USDZ powers Apple AR Quick Look; animated export follows theme 8.",
    dep: "Animation export depends on theme 8.",
    risk: "USDZ packaging specifics.",
  },
  {
    id: "16.1",
    t: "scene",
    title: "Standalone / embeddable web export",
    impact: "High",
    effort: "L",
    fit: "Structural",
    tier: "Near",
    ships: "0.8.1",
    what: "Export a .slxy scene as a self-contained, runnable web bundle (or an embeddable runtime), so authored scenes ship to end users, not just back into the editor.",
    why: "The capability that turns Solarxy from a tool into a platform; the clearest single creative-engine gap, and unusually feasible because the wasm runtime already exists.",
    dep: "Static scenes need only a player; interactive exported scenes depend on theme 7.",
    risk: "Wasm bundle size for a runtime. Start with a static / turntable player, layer interactivity when theme 7 lands.",
  },
  {
    id: "16.2",
    t: "scene",
    title: "Framework integration (embed API)",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "A documented way to embed a Solarxy scene / runtime in a host web app (vanilla, React, Vue), the way mature web engines ship framework templates.",
    why: "Meets developers where they build; multiplies the reach of 16.1.",
    dep: "Depends on 16.1.",
    risk: "API stability commitments.",
  },
  {
    id: "16.3",
    t: "scene",
    title: "Git-friendly text scene format",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "An optional split-file, plain-text scene representation (one file per node, shaders / expressions separate) for version control, alongside the .slxy ZIP.",
    why: "Teams and serious solo users want reviewable history; a ZIP is opaque in a PR.",
    dep: "Independent; reuses the existing schema-owned types.",
    risk: "Keeping two representations in sync via one schema (tractable given schema-owned types).",
  },
  {
    id: "16.4",
    t: "scene",
    title: "Node subnets & reusable groups",
    impact: "Medium",
    effort: "L",
    fit: "Structural",
    tier: "Long",
    what: "Collapse a subgraph into a reusable subnet / asset with its own exposed params, so patterns are packaged and reinstanced.",
    why: "Reuse and scale; the mechanism by which node tools become libraries.",
    dep: "Amplified by 6.1 expressions; the container model is a strong starting point.",
    risk: "Param promotion and versioning of packaged subnets.",
  },
  {
    id: "16.5",
    t: "scene",
    title: "Presets, clipboard & cross-scene copy",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    what: "Copy / paste nodes (and subgraphs) within and across scenes, plus per-node presets.",
    why: "Everyday authoring speed; low glamour, high daily value.",
    dep: "Independent; reuses .slxy fragment encoding.",
    risk: "Reference / asset rebinding on paste across scenes (the engine already refuses reference cycles).",
  },
  {
    id: "17.1",
    t: "ext",
    title: "Plugin / custom-node system",
    impact: "Med-High",
    effort: "L-XL",
    fit: "Structural",
    tier: "Long",
    what: "A way for third parties to add node types without forking, ideally loadable at runtime (wasm plugin modules) or at least via a documented Rust extension crate.",
    why: "Ecosystem growth; lets domains Solarxy will never build itself (CAD, GIS, niche importers) live as plugins.",
    dep: "The registry snapshot already isolates the frontend, a strong foundation; runtime loading is the hard part.",
    risk: "Sandboxing and versioning of third-party wasm; API stability.",
  },
  {
    id: "17.2",
    t: "ext",
    title: "Scripting node (code escape hatch)",
    impact: "Medium",
    effort: "M",
    fit: "Structural",
    tier: "Long",
    what: "A node that runs user code (a constrained expression / script) to compute geometry or values.",
    why: "Power users can solve one-off needs without a plugin; also a testbed for expressions.",
    dep: "Builds on 6.1's evaluator, extended to per-element scope; overlaps 6.2.",
    risk: "Sandboxing; determinism.",
  },
  {
    id: "17.3",
    t: "ext",
    title: "Headless engine API & automation",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Expose the engine as a documented library / CLI so scenes can be cooked, validated, and exported headlessly (batch rendering, CI checks).",
    why: "Automation and pipeline integration; leans directly into Solarxy's CLI / CI-adapter strength.",
    dep: "Reuses the engine and validation crates; the pieces exist, they need an entry point.",
    risk: "Headless rendering needs a wgpu offscreen context (the still-render path is a precedent).",
  },
  {
    id: "18.1",
    t: "internal",
    title: "Wire the native desktop shell to the node engine",
    impact: "High",
    effort: "M + L",
    fit: "Structural",
    tier: "Near",
    split: ["a: engine surface, v0.8.2", "b: node canvas, v0.9.5"],
    what: "Connect the desktop GUI to the node engine so node editing works natively, not only on web. Splits in two: first the engine surface, which hoists scene-environment ownership and adds the Command / EventBatch loop so the desktop opens, cooks and renders .slxy scenes with no canvas; then the canvas, palette, parameter panel and gizmo.",
    why: "Every release since v0.8.0 has been web-only; five of six features in 0.8.1 have no desktop surface. The engine-surface half stops the debt compounding for roughly a tenth of full parity's cost.",
    dep: "Both halves depend on the shared-host extraction, which collapses about 2,000 lines already duplicated between the two hosts. The canvas half was gated on the node-canvas library spike inside v0.8.2, which has now run.",
    risk: "Retired by the spike. egui has no node editor of its own, and the substrate the web canvas library donates (pan, zoom, marquee, edge routing and hit-testing, connection drag) was the real cost of the canvas half. A native substrate was measured against the engine-owns-the-document rule and fits: it can be driven from mirrored state, with node position reconciled the same way the web already reconciles it. Roughly 6,400 lines of groundwork leave the canvas half's scope.",
  },
  {
    id: "18.2",
    t: "internal",
    title: "Activate & grow the expression seam",
    impact: "High",
    effort: "L",
    fit: "Structural",
    tier: "Foundational",
    ships: "0.8.1",
    what: "See 6.1. Called out here because it is a Solarxy-owned reserved seam, not just a capability item; turning it on is a strategic unlock the codebase already anticipates.",
    why: "Cross-cutting leverage for modeling, animation, and scripting.",
    dep: "See 6.1.",
    risk: "See 6.1.",
  },
  {
    id: "18.3",
    t: "internal",
    title: "Grow the wire-type & coercion system",
    impact: "Medium",
    effort: "S",
    fit: "Adaptable",
    tier: "Mid",
    what: "Activate the reserved Light wire type, add a Bundle type for scene-composition wiring when needed, and fill coercion gaps (for example the recorded Color-to-Image follow-up).",
    why: "Several roadmap items want new wire semantics; doing this by the matrix's additive rules keeps it safe.",
    dep: "Enables light-wiring and cross-context texture idioms.",
    risk: "Low by design; the exhaustive snapshot test catches accidents.",
  },
  {
    id: "18.4",
    t: "internal",
    title: "Vertex colors end to end",
    impact: "Medium",
    effort: "M",
    fit: "Adaptable",
    tier: "Mid",
    ships: "0.8.0",
    what: "Carry vertex colors from import through the kernel to the renderer, closing a channel that was repeatedly deferred.",
    why: "Point clouds, scanned data, and many exports rely on vertex color; its absence blocked faithful PLY review.",
    dep: "Pairs with 5.1 attributes; PLY import dropped vertex colors pending this.",
    risk: "Low.",
  },
  {
    id: "18.5",
    t: "internal",
    title: "wasm threads (rayon via COEP) + performance",
    impact: "Medium",
    effort: "M",
    fit: "Structural",
    tier: "Mid",
    what: "Enable multithreading in the wasm build (rayon plus cross-origin isolation) so heavy cooks / imports parallelize, and re-establish a real perf harness.",
    why: "Modeling and simulation cooks will be CPU-bound; threading is the headroom they need.",
    dep: "Unblocks performant modeling and simulation cooks; a minimal performance gate is already planned.",
    risk: "Cross-origin-isolation deployment constraints; the still-in-flux threaded-wasm toolchain.",
  },
  {
    id: "18.6",
    t: "internal",
    title: "WebGL2 fallback for reach",
    impact: "Medium",
    effort: "XL",
    fit: "Research",
    tier: "Aspirational",
    what: "A WebGL2 rendering fallback for browsers / devices without WebGPU, widening the audience for both the editor and any exported runtime.",
    why: "WebGPU coverage is still incomplete; reach matters most for published / exported scenes.",
    dep: "Most valuable alongside 16.1 export.",
    risk: "Maintaining two render backends; feature disparity. Weigh against the WebGPU adoption curve.",
  },
  {
    id: "18.7",
    t: "internal",
    title: "End-to-end & cross-browser test coverage",
    impact: "Medium",
    effort: "M",
    fit: "Native",
    tier: "Mid",
    what: "Add browser-driven end-to-end tests for the web app, an accepted gap today.",
    why: "As the web app grows toward engine breadth, unit tests will not catch integration regressions across the boundary; e2e is the safety net for a solo maintainer shipping live.",
    dep: "Derisks every subsequent web feature.",
    risk: "WebGPU in CI runners limits visual assertions; scope to interaction / flow, not pixels.",
  },
];
