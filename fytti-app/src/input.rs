use winit::event::{ElementState, MouseButton as WinitMouseButton, WindowEvent};
use winit::keyboard::{Key as WinitKey, NamedKey};
use wytti_host::InputEvent;
use wytti_host::{Key, MouseButton, MouseEvent};

/// Convert a winit WindowEvent into a Wytti InputEvent, if applicable.
pub fn convert_event(event: &WindowEvent) -> Option<InputEvent> {
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            let key = convert_key(&event.logical_key)?;
            match event.state {
                ElementState::Pressed => Some(InputEvent::KeyDown(key)),
                ElementState::Released => Some(InputEvent::KeyUp(key)),
            }
        }
        WindowEvent::CursorMoved { position, .. } => Some(InputEvent::MouseMove {
            x: position.x as f32,
            y: position.y as f32,
        }),
        WindowEvent::MouseInput {
            state, button, ..
        } => {
            let btn = match button {
                WinitMouseButton::Left => MouseButton::Left,
                WinitMouseButton::Right => MouseButton::Right,
                WinitMouseButton::Middle => MouseButton::Middle,
                _ => return None,
            };
            Some(InputEvent::MouseClick(MouseEvent {
                x: 0.0, // position comes separately via CursorMoved
                y: 0.0,
                button: btn,
                pressed: *state == ElementState::Pressed,
            }))
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let (dx, dy) = match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => (*x, *y),
                winit::event::MouseScrollDelta::PixelDelta(pos) => {
                    (pos.x as f32, pos.y as f32)
                }
            };
            Some(InputEvent::Scroll { dx, dy })
        }
        WindowEvent::Resized(size) => Some(InputEvent::Resize {
            width: size.width,
            height: size.height,
        }),
        _ => None,
    }
}

fn convert_key(key: &WinitKey) -> Option<Key> {
    match key {
        WinitKey::Named(named) => match named {
            NamedKey::ArrowUp => Some(Key::Up),
            NamedKey::ArrowDown => Some(Key::Down),
            NamedKey::ArrowLeft => Some(Key::Left),
            NamedKey::ArrowRight => Some(Key::Right),
            NamedKey::Space => Some(Key::Space),
            NamedKey::Enter => Some(Key::Enter),
            NamedKey::Escape => Some(Key::Escape),
            NamedKey::Backspace => Some(Key::Backspace),
            NamedKey::Tab => Some(Key::Tab),
            _ => None,
        },
        WinitKey::Character(c) => {
            let ch = c.chars().next()?;
            Some(Key::Char(ch))
        }
        _ => None,
    }
}
