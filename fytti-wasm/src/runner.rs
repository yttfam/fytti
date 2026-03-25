use std::sync::Arc;

use anyhow::Result;
use wasmtime::{Caller, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};
use wasmtime_wasi::preview1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;
use wytti_host::{Color, HostBackend, Rect, ResourceId};

use crate::backend::FyttiBackend;

/// Combined WASM store state: WASI P1 context + Fytti rendering backend.
pub(crate) struct FyttiWasmState {
    pub backend: FyttiBackend,
    wasi: WasiP1Ctx,
    limits: StoreLimits,
}

/// Runs WASM guest apps with Fytti as the rendering host.
pub struct WasmRunner {
    engine: Arc<Engine>,
}

impl WasmRunner {
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.epoch_interruption(true);
        let engine = Engine::new(&config)?;
        Ok(Self {
            engine: Arc::new(engine),
        })
    }

    /// Load a WASM file and create a ready-to-run WasmApp.
    pub fn load(&self, path: &str, width: u32, height: u32) -> Result<WasmApp> {
        let module = Module::from_file(&self.engine, path)?;
        let backend = FyttiBackend::new(width, height);

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |state: &mut FyttiWasmState| {
            &mut state.wasi
        })?;
        add_fytti_functions(&mut linker)?;

        let limits = StoreLimitsBuilder::new()
            .memory_size(256 * 1024 * 1024)
            .build();
        let wasi = WasiCtxBuilder::new().inherit_stdio().build_p1();
        let state = FyttiWasmState {
            backend,
            wasi,
            limits,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_epoch_deadline(60);
        store.epoch_deadline_trap();

        let ticker = self.spawn_epoch_ticker();

        // Instantiate once — this instance lives for the app's lifetime
        let instance = linker.instantiate(&mut store, &module)?;

        // Run _start (init)
        if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
            start.call(&mut store, ())?;
        }

        // Grab frame() handle if it exists
        let frame_fn = instance
            .get_typed_func::<(), ()>(&mut store, "frame")
            .ok();

        // Check if _start already presented (static app with no frame export)
        let already_presented = store.data().backend.presented;

        Ok(WasmApp {
            store,
            _ticker: ticker,
            frame_fn,
            already_presented,
        })
    }

    fn spawn_epoch_ticker(&self) -> std::thread::JoinHandle<()> {
        let engine = Arc::clone(&self.engine);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            engine.increment_epoch();
        })
    }
}

/// A loaded WASM app ready to run frames.
///
/// Instance persists between frame() calls — guest state in linear memory
/// (static mut variables, heap allocations) survives across frames.
pub struct WasmApp {
    store: Store<FyttiWasmState>,
    _ticker: std::thread::JoinHandle<()>,
    frame_fn: Option<TypedFunc<(), ()>>,
    already_presented: bool,
}

impl WasmApp {
    /// Run one frame. Returns true if the guest requested another frame.
    pub fn run_frame(&mut self) -> Result<bool> {
        if let Some(frame_fn) = &self.frame_fn {
            // Reset per-frame state
            self.store.data_mut().backend.frame_requested = false;
            self.store.data_mut().backend.presented = false;
            let (w, h) = {
                let b = &self.store.data().backend;
                (b.display_list.width, b.display_list.height)
            };
            self.store.data_mut().backend.display_list.reset(w, h);

            // Reset epoch deadline per frame (per Wytti's recommendation)
            self.store.set_epoch_deadline(5);

            frame_fn.call(&mut self.store, ())?;

            Ok(self.store.data().backend.frame_requested)
        } else {
            // Static app: _start already drew everything. No more frames.
            Ok(false)
        }
    }

    /// Whether init already presented a frame (static app).
    pub fn init_presented(&self) -> bool {
        self.already_presented
    }

    /// Get the display list from the last frame.
    pub fn display_list(&self) -> &fytti_render::display_list::DisplayList {
        &self.store.data().backend.display_list
    }

    /// Whether a frame was presented (guest called present()).
    pub fn presented(&self) -> bool {
        self.store.data().backend.presented
    }

    /// Whether the guest wants another frame.
    pub fn frame_requested(&self) -> bool {
        self.store.data().backend.frame_requested
    }

    /// Get the window title set by the guest.
    pub fn title(&self) -> &str {
        &self.store.data().backend.title
    }

    /// Resize the rendering backend.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.store.data_mut().backend.resize(width, height);
    }

    /// Push an input event into the guest's event queue.
    pub fn push_event(&mut self, event: wytti_host::InputEvent) {
        self.store.data_mut().backend.push_event(event);
    }

    /// Add FPS overlay commands to the display list.
    pub fn add_fps_overlay(&mut self, fps_text: &str) {
        use fytti_render::display_list::{color_to_f32, DrawCmd};
        let w = self.store.data().backend.display_list.width as f32;
        let dl = &mut self.store.data_mut().backend.display_list;
        dl.commands.push(DrawCmd::FillRect {
            x: w - 80.0, y: 4.0, w: 76.0, h: 22.0,
            color: color_to_f32(0, 0, 0, 180),
        });
        dl.commands.push(DrawCmd::Text {
            text: fps_text.to_string(),
            x: w - 74.0, y: 5.0, size: 14.0,
            color: color_to_f32(0, 255, 100, 255),
        });
    }
}

// ── Register fytti_* host functions on the linker ──

fn read_guest_string(
    caller: &mut Caller<'_, FyttiWasmState>,
    ptr: u32,
    len: u32,
) -> Option<String> {
    let mem = caller.get_export("memory")?.into_memory()?;
    let data = mem.data(&*caller);
    let start = ptr as usize;
    let end = start + len as usize;
    if end > data.len() {
        return None;
    }
    std::str::from_utf8(&data[start..end])
        .ok()
        .map(|s| s.to_string())
}

/// Pack an InputEvent into a u64 for the WASM ABI.
/// Format: [type:8][data:56]
fn pack_event(event: wytti_host::InputEvent) -> u64 {
    use wytti_host::InputEvent;
    match event {
        InputEvent::KeyDown(key) => 1u64 << 56 | pack_key(key) as u64,
        InputEvent::KeyUp(key) => 2u64 << 56 | pack_key(key) as u64,
        InputEvent::MouseMove { x, y } => {
            3u64 << 56 | (x.to_bits() as u64) << 24 | (y.to_bits() as u64 & 0xFFFFFF)
        }
        InputEvent::MouseClick(me) => {
            let pressed = if me.pressed { 1u64 } else { 0u64 };
            4u64 << 56 | (me.button as u64) << 48 | pressed << 40
        }
        InputEvent::Scroll { .. } => 5u64 << 56,
        InputEvent::Resize { width, height } => {
            6u64 << 56 | (width as u64) << 24 | height as u64
        }
    }
}

fn pack_key(key: wytti_host::Key) -> u32 {
    use wytti_host::Key;
    match key {
        Key::Up => 1,
        Key::Down => 2,
        Key::Left => 3,
        Key::Right => 4,
        Key::Space => 5,
        Key::Enter => 6,
        Key::Escape => 7,
        Key::Backspace => 8,
        Key::Tab => 9,
        Key::Char(c) => 0x100 | c as u32,
    }
}

fn add_fytti_functions(linker: &mut Linker<FyttiWasmState>) -> Result<()> {
    linker.func_wrap("fytti", "clear", |mut caller: Caller<'_, FyttiWasmState>, color: u32| {
        caller.data_mut().backend.clear(Color::from_u32(color));
    })?;

    linker.func_wrap("fytti", "fill_rect",
        |mut caller: Caller<'_, FyttiWasmState>, x: f32, y: f32, w: f32, h: f32, color: u32| {
            caller.data_mut().backend.fill_rect(Rect::new(x, y, w, h), Color::from_u32(color));
        },
    )?;

    linker.func_wrap("fytti", "stroke_rect",
        |mut caller: Caller<'_, FyttiWasmState>, x: f32, y: f32, w: f32, h: f32, color: u32, width: f32| {
            caller.data_mut().backend.stroke_rect(Rect::new(x, y, w, h), Color::from_u32(color), width);
        },
    )?;

    linker.func_wrap("fytti", "draw_line",
        |mut caller: Caller<'_, FyttiWasmState>, x1: f32, y1: f32, x2: f32, y2: f32, color: u32, width: f32| {
            caller.data_mut().backend.draw_line(x1, y1, x2, y2, Color::from_u32(color), width);
        },
    )?;

    linker.func_wrap("fytti", "draw_text",
        |mut caller: Caller<'_, FyttiWasmState>, ptr: u32, len: u32, x: f32, y: f32, size: f32, font_id: u32, color: u32| {
            if let Some(text) = read_guest_string(&mut caller, ptr, len) {
                caller.data_mut().backend.draw_text(&text, x, y, size, ResourceId(font_id), Color::from_u32(color));
            }
        },
    )?;

    linker.func_wrap("fytti", "draw_image",
        |mut caller: Caller<'_, FyttiWasmState>, image_id: u32, x: f32, y: f32, w: f32, h: f32| {
            caller.data_mut().backend.draw_image(ResourceId(image_id), x, y, w, h);
        },
    )?;

    linker.func_wrap("fytti", "present", |mut caller: Caller<'_, FyttiWasmState>| {
        caller.data_mut().backend.present();
    })?;

    linker.func_wrap("fytti", "poll_event", |mut caller: Caller<'_, FyttiWasmState>| -> u64 {
        match caller.data_mut().backend.poll_event() {
            None => 0,
            Some(event) => pack_event(event),
        }
    })?;

    linker.func_wrap("fytti", "load_font",
        |mut caller: Caller<'_, FyttiWasmState>, ptr: u32, len: u32| -> u32 {
            if let Some(name) = read_guest_string(&mut caller, ptr, len) {
                caller.data_mut().backend.load_font(&name).0
            } else {
                0
            }
        },
    )?;

    linker.func_wrap("fytti", "load_image",
        |mut caller: Caller<'_, FyttiWasmState>, ptr: u32, len: u32| -> u32 {
            if let Some(url) = read_guest_string(&mut caller, ptr, len) {
                caller.data_mut().backend.load_image(&url).0
            } else {
                0
            }
        },
    )?;

    linker.func_wrap("fytti", "set_title",
        |mut caller: Caller<'_, FyttiWasmState>, ptr: u32, len: u32| {
            if let Some(title) = read_guest_string(&mut caller, ptr, len) {
                caller.data_mut().backend.set_title(&title);
            }
        },
    )?;

    linker.func_wrap("fytti", "request_frame", |mut caller: Caller<'_, FyttiWasmState>| {
        caller.data_mut().backend.request_frame();
    })?;

    // --- Extended drawing API ---

    // gradient_rect(x, y, w, h, color1, color2, vertical: u32)
    linker.func_wrap("fytti", "gradient_rect",
        |mut caller: Caller<'_, FyttiWasmState>, x: f32, y: f32, w: f32, h: f32, color1: u32, color2: u32, vertical: u32| {
            caller.data_mut().backend.gradient_rect(
                Rect::new(x, y, w, h), Color::from_u32(color1), Color::from_u32(color2), vertical != 0,
            );
        },
    )?;

    // fill_ellipse(cx, cy, rx, ry, color)
    linker.func_wrap("fytti", "fill_ellipse",
        |mut caller: Caller<'_, FyttiWasmState>, cx: f32, cy: f32, rx: f32, ry: f32, color: u32| {
            caller.data_mut().backend.fill_ellipse(cx, cy, rx, ry, Color::from_u32(color));
        },
    )?;

    // stroke_ellipse(cx, cy, rx, ry, color, width)
    linker.func_wrap("fytti", "stroke_ellipse",
        |mut caller: Caller<'_, FyttiWasmState>, cx: f32, cy: f32, rx: f32, ry: f32, color: u32, width: f32| {
            // stroke_ellipse is a Fytti extension, push directly to display list
            use fytti_render::display_list::{DrawCmd, color_to_f32};
            let c = Color::from_u32(color);
            caller.data_mut().backend.display_list.commands.push(DrawCmd::StrokeEllipse {
                cx, cy, rx, ry,
                color: color_to_f32(c.r, c.g, c.b, c.a),
                width,
            });
        },
    )?;

    // poll_mouse() -> u64 (packed: x as f32 bits in high 32, y as f32 bits in low 32; 0 = no update)
    linker.func_wrap("fytti", "poll_mouse",
        |mut caller: Caller<'_, FyttiWasmState>| -> u64 {
            match caller.data_mut().backend.poll_mouse() {
                None => 0,
                Some((x, y)) => {
                    let xbits = x.to_bits() as u64;
                    let ybits = y.to_bits() as u64;
                    (xbits << 32) | ybits
                }
            }
        },
    )?;

    // Viewport size — so guests can scale to window dimensions
    linker.func_wrap("fytti", "get_width", |caller: Caller<'_, FyttiWasmState>| -> u32 {
        caller.data().backend.get_width()
    })?;

    linker.func_wrap("fytti", "get_height", |caller: Caller<'_, FyttiWasmState>| -> u32 {
        caller.data().backend.get_height()
    })?;

    Ok(())
}
