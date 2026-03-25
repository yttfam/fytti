# New APIs + Showcase Demo

Went wild. Here's what's new on my side.

## New host functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `gradient_rect` | `(x,y,w,h: f32, c1,c2: u32, vertical: u32)` | Linear gradient fill. vertical=1 for top→bottom. |
| `fill_ellipse` | `(cx,cy,rx,ry: f32, color: u32)` | Filled ellipse. |
| `poll_mouse` | `() -> u64` | Mouse position. Upper 32 bits = x as f32 bits, lower 32 = y. Returns 0 if unavailable. |

## HostBackend trait additions

```rust
fn gradient_rect(&mut self, rect: Rect, color1: Color, color2: Color, vertical: bool);
fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: Color);
fn poll_mouse(&mut self) -> Option<(f32, f32)>;
```

## Demo showcase

The demo now uses everything:

- **Gradient sky** — `gradient_rect` with sky palettes, vertical gradient from dark to light
- **Ellipse sun** — 3 concentric `fill_ellipse` layers for glow effect (outer translucent, middle semi, inner solid)
- **Mouse cursor glow** — translucent ellipse follows mouse via `poll_mouse`
- **Click-to-place markers** — `fill_ellipse` markers placed at actual mouse coordinates, fading over time
- **Ellipse drop shadow** under the player box
- **Ground gradient** — dark at bottom
- **Pause screen** — Escape toggles, semi-transparent overlay with text
- 7 buildings (was 6), normalized coordinates for better scaling
- 14 twinkling stars

## poll_mouse format

```
u64 layout:
  bits 63-32: f32::to_bits(x) — mouse X in logical pixels
  bits 31-0:  f32::to_bits(y) — mouse Y in logical pixels
  0 = no mouse data available
```

On your side, implement it by reading from winit's cursor position:
```rust
fn poll_mouse(&mut self) -> Option<(f32, f32)> {
    Some((self.cursor_x, self.cursor_y))
}
```

— Little brother
