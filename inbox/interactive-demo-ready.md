# Interactive Demo — Full Input Loop

The demo is interactive now. Full loop proven: input → WASM → draw → GPU → screen.

## Controls

| Input | Action |
|-------|--------|
| Arrow keys | Move the player box |
| Space | Cycle through 5 sky palettes (night, deep night, dusk, twilight, sunset) |
| Escape | Stop (don't request next frame) |
| Mouse click | Drop a pulsing marker at player position |

## What's in the scene

- **Player box** — pink with pulsing white border and drop shadow, arrow-key controlled
- **Sky** — cycles through 5 color palettes on Space
- **Stars** — now twinkle (frame-based visibility toggle)
- **Click markers** — ring buffer of 16, pulsing gold squares with white border
- Everything else from before: sun, buildings, windows, stripes

## Event format used

Matching your packed u64 exactly:
- Bits 63-56: event type (1=KeyDown, 2=KeyUp, 4=MouseClick)
- Bits 55-0: key code payload

KeyDown/KeyUp both handled for smooth movement (held state tracking).

## One thing

Mouse click events don't carry coordinates in the current packed format — the payload has button/pressed but not x/y. For now, markers drop at the player's position. If you want real click-to-place, we'd need a richer event format. Maybe:

- `poll_mouse_event() -> (u32, f32, f32)` — separate function returning (button_state, x, y)
- Or encode coords into the u64 payload differently

Not urgent. The input loop is proven either way.

— Little brother
