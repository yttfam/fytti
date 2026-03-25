You are building Fytti — a minimal, opinionated browser engine in Rust. The fifth and most ambitious child of the YTT dynasty. The name stands for what it stands for. You know.

Fytti doesn't try to be Chrome. Fytti doesn't try to pass Acid3. Fytti renders the web the way the web should be — fast, lean, and without 40 million lines of C++ committee-designed bloat.

## The YTT Family

- **Hermytt** — terminal multiplexer (transport layer)
- **Crytter** — WASM terminal emulator (Canvas2D rendering)
- **Prytty** — syntax highlighting (text beautification)
- **Wytti** — WASI runtime (sandboxed execution)
- **Fytti** — browser engine (the final boss)

## The Full Stack Realized

```
Fytti (browser shell)
├── HTML/CSS parser → layout engine → renderer
├── Crytter (terminal emulator embedded as a component)
├── Prytty (view-source highlighting, devtools)
├── Wytti (WASM execution for web apps)
└── Hermytt (terminal-in-browser, remote sessions)
```

Every sibling becomes a component. Fytti is the shell that holds them together.

## Philosophy

1. **Render HTML and CSS correctly enough** — not perfectly, correctly enough. 90% of the web uses 20% of the spec. Start there.
2. **No JavaScript engine** — WASM only (via Wytti). JS is the disease, WASM is the cure. Sites that require JS get a polite "upgrade to WASM."
3. **No 500 W3C specs** — implement what matters: HTML5 semantic elements, CSS flexbox/grid, forms, links, images, video. Skip the rest.
4. **Security by architecture** — every page is a sandboxed WASI instance. No shared state. No cookies across domains (unless explicit). No fingerprinting surface.
5. **Performance by simplicity** — fewer features = less code = faster rendering. A page that loads in 10ms because we skip the JS runtime, the extension API, the sync engine, the telemetry, the...

## Architecture

```
fytti/
├── fytti-html/           # HTML5 parser → DOM tree
├── fytti-css/            # CSS parser → CSSOM, cascade, specificity
├── fytti-layout/         # Layout engine: block, inline, flexbox, grid
├── fytti-render/         # Renderer: GPU (wgpu) or software (tiny-skia)
├── fytti-net/            # HTTP client, TLS, caching, connection pooling
├── fytti-dom/            # DOM API exposed to WASM via Wytti
├── fytti-wasm/           # Wytti integration: WASM instead of JS
├── fytti-ui/             # Chrome/shell: address bar, tabs, bookmarks
├── fytti-devtools/       # Inspector: DOM tree, CSS, network (uses Prytty for highlighting)
└── fytti-app/            # Native app: winit + wgpu window, or Tauri shell
```

## Rendering Pipeline

```
HTML bytes
  → fytti-html (parse) → DOM tree
  → fytti-css (parse + cascade) → styled DOM
  → fytti-layout (box model, flexbox, grid) → layout tree
  → fytti-render (paint) → pixels on screen
```

No reflow storms. No layout thrashing. Parse once, lay out once, paint once. Re-render only what changed.

## What Fytti Supports (MVP)

### HTML
- Semantic elements: div, span, p, h1-h6, a, img, video, audio
- Forms: input, textarea, select, button
- Tables: table, tr, td, th
- Lists: ul, ol, li
- Sections: header, footer, nav, main, article, section
- head, meta, title, link (CSS), style

### CSS
- Selectors: element, class, id, descendant, child, pseudo-classes (:hover, :focus, :first-child)
- Box model: margin, padding, border, width, height
- Display: block, inline, flex, grid, none
- Positioning: static, relative, absolute, fixed
- Colors, fonts, text properties
- Media queries (viewport-based)
- Variables (custom properties)
- Transitions (basic)

### NOT Supported (by design)
- JavaScript (use WASM)
- Canvas 2D/WebGL (Crytter handles terminal canvas, Wytti handles compute)
- Web Components / Shadow DOM
- Service Workers
- WebRTC
- 90% of the Web API surface that only exists because browsers became operating systems

## The JS Question

No JavaScript. Period. This is the hill Fytti dies on.

- Sites with WASM: fully supported via Wytti
- Static sites: perfect rendering
- Sites requiring JS: display a banner "This site requires JavaScript. Fytti supports WASM. The future is now."
- Progressive enhancement: if a site works without JS (as it should), Fytti renders it beautifully

This is not a limitation. This is a statement.

## Existing Art

- **Servo** (Mozilla/Linux Foundation) — Rust browser engine, parallel layout, real but complex
- **Ladybird** (SerenityOS) — C++, from-scratch browser, similar philosophy
- **Dillo** — tiny C browser, basic CSS, fast, abandoned then revived
- **NetSurf** — C, small, good CSS, active

Fytti learns from all of them. Servo's parallel layout ideas. Ladybird's "just build it" attitude. Dillo's minimalism. But Fytti has something none of them have: a family of Rust siblings that handle terminal, WASM, and text rendering already.

## Tech Stack

- `html5ever` — HTML parser (from Servo, battle-tested)
- `cssparser` — CSS parser (also from Servo)
- `wgpu` — GPU rendering (cross-platform Vulkan/Metal/DX12)
- `tiny-skia` — software rendering fallback
- `winit` — windowing (cross-platform)
- `reqwest` — HTTP client
- `rustls` — TLS
- `image` — image decoding
- `wasmtime` — via Wytti for WASM execution
- `font-kit` or `cosmic-text` — font loading and text shaping

## Milestones

### v0.1 — "It renders"
- HTML parser → DOM
- CSS parser → styles
- Block layout only
- Software renderer
- Render a static HTML page correctly

### v0.2 — "It's usable"
- Flexbox layout
- Links and navigation
- Images
- Forms (basic)
- Address bar, back/forward

### v0.3 — "It's a browser"
- Grid layout
- Tabs
- Bookmarks
- WASM support via Wytti
- Devtools via Prytty

### v1.0 — "Fuck your terrible tech infrastructure"
- GPU rendering
- Hermytt terminal embedded
- Crytter for terminal-in-browser
- Full YTT family integration
- Ship it

## Cali's Preferences

- Start with the rendering pipeline — parse HTML, lay out boxes, paint pixels
- html5ever and cssparser from Servo — don't reinvent the parser
- Software rendering first (tiny-skia), GPU later
- Cross-platform from day one (winit)
- This is the long game — won't ship in a day, and that's fine
- The name is Fytti. It means what it means. The browser that says no.
