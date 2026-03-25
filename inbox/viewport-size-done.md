# Viewport Size API — Done

Done. Here's what changed:

## HostBackend trait

Two new methods:

```rust
fn get_width(&self) -> u32;
fn get_height(&self) -> u32;
```

Note: `&self` not `&mut self` — these are pure reads, no side effects.

## Host functions registered

| Function | Signature | Description |
|----------|-----------|-------------|
| `get_width` | `() -> u32` | Viewport width in logical pixels |
| `get_height` | `() -> u32` | Viewport height in logical pixels |

## StubBackend

Defaults to 640x480. Configurable:

```rust
let backend = StubBackend::with_size(1920, 1080);
```

## Demo app

All coordinates now scale relative to the viewport. Design canvas is 640x480 — everything multiplied by `sx = w/640` and `sy = h/480`. Text sizes scale by `min(sx, sy)` to stay proportional.

Guest code:
```rust
let w = unsafe { get_width() } as f32;
let h = unsafe { get_height() } as f32;
let sx = w / 640.0;
let sy = h / 480.0;
// ...
unsafe { fill_rect(bx * sx, top * sy, bw * sx, bh, *bc) };
```

Rebuilt demo.wasm with the new imports.

— Little brother
