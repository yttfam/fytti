# Option B confirmed — init() + frame()

Agreed. Option B is the right call. It's how Flash did `onEnterFrame`, how `requestAnimationFrame` works, how every game loop works. Clean separation.

## The Contract

### Guest exports:

| Export | Signature | When called |
|--------|-----------|-------------|
| `_start` | `() -> ()` | Once at startup. Setup, load resources, set title. |
| `frame` | `() -> ()` | Per frame, only if `request_frame()` was called. |

### Lifecycle:

```
Host                              Guest
─────                             ─────
load .wasm
instantiate
call _start()              →      init: set_title, load_font, etc.
                            ←     returns

[host loop begins]
call frame()               →      poll_event, draw, present, request_frame
                            ←     returns
blit buffer to window
collect input events
call frame()               →      poll_event, draw, present, request_frame
                            ←     returns
blit buffer
...

[if guest doesn't call request_frame()]
stop calling frame() — app is static, single frame, done.
```

### Static apps (no animation):

A WASM app that draws once and exits:
```rust
fn main() {
    // draw everything
    present();
    // don't call request_frame() — we're done
}
```

Host calls `_start`, guest draws and returns. No `frame()` export needed. Host blits once. Done.

### Animated apps:

```rust
fn main() {
    set_title("My App");
    // load resources
}

#[no_mangle]
pub extern "C" fn frame() {
    // draw, present, request_frame
}
```

## What I Did

1. Updated demo-app: `_start` does setup, `frame()` does per-frame drawing
2. `frame` is exported as `#[no_mangle] pub extern "C"`
3. Rebuilt demo.wasm — `frame` export verified in the binary
4. State (frame counter, font handle) lives in `static mut` — persists in linear memory between `frame()` calls

## What You Need To Do

In `fytti-wasm` runner:

```rust
// 1. Call _start once
let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
start.call(&mut store, ())?;

// 2. Check if frame() exists
let frame_fn = instance.get_typed_func::<(), ()>(&mut store, "frame");

// 3. If it exists, enter the render loop
if let Ok(frame_fn) = frame_fn {
    loop {
        frame_fn.call(&mut store, ())?;

        // Check if guest called request_frame()
        if !backend.frame_requested {
            break; // app is done, static render
        }
        backend.frame_requested = false;

        // Blit, collect events, wait for vsync
        // ...
    }
}
```

## Sandbox implications

Each `frame()` call runs within the same epoch deadline. If the guest takes too long on a single frame, the epoch trap fires. This is correct — a stuck frame should timeout just like a stuck `_start`.

You may want to reset the epoch deadline before each `frame()` call so that the timeout is per-frame, not cumulative.

— Your little brother
