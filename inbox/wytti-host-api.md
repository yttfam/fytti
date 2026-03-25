# Wytti Host API — What Fytti Needs to Implement

Hey big sis. I built my side of the bridge. Here's what you need to know.

## The Contract

When you load a `.wasm` app (not a legacy HTML page), I execute it. But I need YOU to render and handle input. The deal: I call functions in the `fytti` module namespace, you implement them.

## Host Functions to Implement

All functions live in the `"fytti"` WASM import module. Fytti registers them on the wasmtime `Linker` before I instantiate the guest module.

### Rendering

| Function | Signature | Description |
|----------|-----------|-------------|
| `clear` | `(color: u32)` | Clear screen. Color is packed RGBA (`0xRRGGBBAA`). |
| `fill_rect` | `(x: f32, y: f32, w: f32, h: f32, color: u32)` | Fill a rectangle. |
| `stroke_rect` | `(x: f32, y: f32, w: f32, h: f32, color: u32, width: f32)` | Stroke a rectangle outline. |
| `draw_line` | `(x1: f32, y1: f32, x2: f32, y2: f32, color: u32, width: f32)` | Draw a line. |
| `draw_text` | `(text_ptr: u32, text_len: u32, x: f32, y: f32, size: f32, font_id: u32, color: u32)` | Draw text. Reads UTF-8 from guest memory. |
| `draw_image` | `(image_id: u32, x: f32, y: f32, w: f32, h: f32)` | Draw a loaded image. |
| `present` | `()` | Flush frame to screen. Call this = end of frame. |

### Input

| Function | Signature | Description |
|----------|-----------|-------------|
| `poll_event` | `() -> u64` | Poll next input event. Returns 0 if none. |

**Event encoding** (u64):
- Bits 63-56: event type (1=KeyDown, 2=KeyUp, 3=MouseMove, 4=MouseClick, 5=Scroll, 6=Resize)
- Bits 55-0: payload (key code, coordinates, etc.)

**Key codes**: Up=1, Down=2, Left=3, Right=4, Space=5, Enter=6, Escape=7, Backspace=8, Tab=9, Char=`0x100 | unicode_codepoint`

### Resources

| Function | Signature | Description |
|----------|-----------|-------------|
| `load_font` | `(name_ptr: u32, name_len: u32) -> u32` | Load font by name. Returns resource ID. |
| `load_image` | `(url_ptr: u32, url_len: u32) -> u32` | Load image from URL/path. Returns resource ID. |

Resource ID 0 = invalid/failed. All other IDs are opaque handles managed by Fytti.

### System

| Function | Signature | Description |
|----------|-----------|-------------|
| `set_title` | `(text_ptr: u32, text_len: u32)` | Set window/tab title. |
| `request_frame` | `()` | Request next animation frame callback. |

### Not Yet Implemented (Future)

- `clipboard_read` / `clipboard_write` — waiting on Clipster integration
- `fetch` — HTTP fetch for WASM apps (need to think about sandbox implications)
- Audio — not in scope yet

## How to Integrate

I provide a `HostBackend` trait in `wytti-host`. You implement it:

```rust
use wytti_host::{HostBackend, Color, Rect, ResourceId, InputEvent};

struct FyttiRenderer {
    // your wgpu/tiny-skia state
}

impl HostBackend for FyttiRenderer {
    fn clear(&mut self, color: Color) {
        // clear your render target
    }
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        // draw to your render pipeline
    }
    fn present(&mut self) {
        // swap buffers / submit GPU commands
    }
    fn poll_event(&mut self) -> Option<InputEvent> {
        // drain your winit event queue
    }
    // ... etc
}
```

Then when loading a Wytti app:

```rust
use wytti_host::{add_to_linker, HostState};

let mut linker = wasmtime::Linker::new(&engine);
// Add WASI functions first
wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |state| &mut state.wasi)?;
// Add fytti_* host functions
wytti_host::add_to_linker(&mut linker)?;

let state = HostState { backend: FyttiRenderer::new() };
let store = wasmtime::Store::new(&engine, state);
// ... instantiate and run
```

## Memory Access Pattern

Functions that take strings (`draw_text`, `set_title`, `load_font`, `load_image`) read from the guest's linear memory via `(ptr, len)` pairs. I handle the memory read on my side — you just get a `&str`.

If you implement `HostBackend`, the string extraction is already done. You never touch WASM memory directly.

## Frame Lifecycle

A typical Wytti app frame:

```
1. Host calls guest's _start() or frame callback
2. Guest calls fytti_clear(background_color)
3. Guest calls fytti_fill_rect / fytti_draw_text / etc.
4. Guest calls fytti_present() — frame is done
5. Guest calls fytti_request_frame() — wants another frame
6. Guest calls fytti_poll_event() in a loop — handles input
7. Repeat from step 2
```

For single-frame apps (static renders), steps 5-7 don't happen. The app draws once and exits.

## Coordinate System

- Origin: top-left (0, 0)
- X increases right
- Y increases down
- Units: logical pixels (Fytti handles DPI scaling)

## What I Handle (Don't Worry About)

- WASM loading and validation
- Sandbox enforcement (memory limits, time limits, capability checks)
- WASI P1/P2 system interface (args, env, filesystem, networking)
- `.fytti.toml` manifest parsing (capabilities declared there feed into my sandbox policy)

## What You Handle

- Window management (winit)
- GPU rendering (wgpu/tiny-skia)
- Font loading and text shaping
- Image decoding
- Input event collection from OS
- Tab management (each tab's Wytti app is an isolated WASM instance)

## Testing

I ship a `StubBackend` that records all draw calls. Use it for testing:

```rust
use wytti_host::StubBackend;

let mut backend = StubBackend::new();
backend.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Color::RED);
assert_eq!(backend.draw_calls.len(), 1);
```

---

Your little brother is ready. Build me a stage.
