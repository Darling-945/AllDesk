//! macOS input controller using CoreGraphics CGEvent API.
//!
//! Requires the calling process to have accessibility permissions:
//! System Settings > Privacy & Security > Accessibility

use std::sync::Mutex;

use alldesk_core::Error;
use core_graphics::event::{
    CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, KeyCode as Vk, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use tracing::{instrument, warn};

use crate::input::controller::{ButtonState, InputController, KeyCode, KeyState, MouseButton};

pub struct MacInputController {
    source: CGEventSource,
    cursor_pos: Mutex<CGPoint>,
}

impl MacInputController {
    pub fn new() -> Self {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .expect("Failed to create CGEventSource");
        Self {
            source,
            cursor_pos: Mutex::new(CGPoint::new(0.0, 0.0)),
        }
    }
}

impl Default for MacInputController {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for MacInputController {}
unsafe impl Sync for MacInputController {}

fn key_code_to_vk(key: &KeyCode) -> Option<u16> {
    match key {
        KeyCode::Char(c) => char_to_vk(*c),
        KeyCode::Enter => Some(Vk::RETURN),
        KeyCode::Escape => Some(Vk::ESCAPE),
        KeyCode::Tab => Some(Vk::TAB),
        KeyCode::Backspace => Some(Vk::DELETE),
        KeyCode::Delete => Some(Vk::FORWARD_DELETE),
        KeyCode::ArrowUp => Some(Vk::UP_ARROW),
        KeyCode::ArrowDown => Some(Vk::DOWN_ARROW),
        KeyCode::ArrowLeft => Some(Vk::LEFT_ARROW),
        KeyCode::ArrowRight => Some(Vk::RIGHT_ARROW),
        KeyCode::Space => Some(Vk::SPACE),
        KeyCode::Home => Some(Vk::HOME),
        KeyCode::End => Some(Vk::END),
        KeyCode::PageUp => Some(Vk::PAGE_UP),
        KeyCode::PageDown => Some(Vk::PAGE_DOWN),
        KeyCode::Insert => None, // no direct macOS equivalent
        KeyCode::PrintScreen => None,
        KeyCode::Pause => None,
        KeyCode::CapsLock => Some(Vk::CAPS_LOCK),
        KeyCode::NumLock => None,
        KeyCode::ScrollLock => None,
        KeyCode::Menu => None,
        KeyCode::LeftCtrl => Some(Vk::CONTROL),
        KeyCode::RightCtrl => Some(Vk::RIGHT_CONTROL),
        KeyCode::LeftShift => Some(Vk::SHIFT),
        KeyCode::RightShift => Some(Vk::RIGHT_SHIFT),
        KeyCode::LeftAlt => Some(Vk::OPTION),
        KeyCode::RightAlt => Some(Vk::RIGHT_OPTION),
        KeyCode::LeftMeta => Some(Vk::COMMAND),
        KeyCode::RightMeta => Some(Vk::RIGHT_COMMAND),
        KeyCode::Function(n) => match *n {
            1 => Some(Vk::F1),
            2 => Some(Vk::F2),
            3 => Some(Vk::F3),
            4 => Some(Vk::F4),
            5 => Some(Vk::F5),
            6 => Some(Vk::F6),
            7 => Some(Vk::F7),
            8 => Some(Vk::F8),
            9 => Some(Vk::F9),
            10 => Some(Vk::F10),
            11 => Some(Vk::F11),
            12 => Some(Vk::F12),
            13 => Some(Vk::F13),
            14 => Some(Vk::F14),
            15 => Some(Vk::F15),
            16 => Some(Vk::F16),
            17 => Some(Vk::F17),
            18 => Some(Vk::F18),
            19 => Some(Vk::F19),
            20 => Some(Vk::F20),
            _ => None,
        },
        KeyCode::Unknown(_) => None,
    }
}

/// ANSI printable-key virtual key codes (HID usage keyboard, from Carbon
/// Events.h). core-graphics 0.24 only wraps the special keys, so the
/// printable ones are defined here.
mod ansi {
    pub const A: u16 = 0x00;
    pub const S: u16 = 0x01;
    pub const D: u16 = 0x02;
    pub const F: u16 = 0x03;
    pub const H: u16 = 0x04;
    pub const G: u16 = 0x05;
    pub const Z: u16 = 0x06;
    pub const X: u16 = 0x07;
    pub const C: u16 = 0x08;
    pub const V: u16 = 0x09;
    pub const B: u16 = 0x0B;
    pub const Q: u16 = 0x0C;
    pub const W: u16 = 0x0D;
    pub const E: u16 = 0x0E;
    pub const R: u16 = 0x0F;
    pub const Y: u16 = 0x10;
    pub const T: u16 = 0x11;
    pub const KEY_1: u16 = 0x12;
    pub const KEY_2: u16 = 0x13;
    pub const KEY_3: u16 = 0x14;
    pub const KEY_4: u16 = 0x15;
    pub const KEY_6: u16 = 0x16;
    pub const KEY_5: u16 = 0x17;
    pub const EQUAL: u16 = 0x18;
    pub const KEY_9: u16 = 0x19;
    pub const KEY_7: u16 = 0x1A;
    pub const MINUS: u16 = 0x1B;
    pub const KEY_8: u16 = 0x1C;
    pub const KEY_0: u16 = 0x1D;
    pub const RIGHT_BRACKET: u16 = 0x1E;
    pub const O: u16 = 0x1F;
    pub const U: u16 = 0x20;
    pub const LEFT_BRACKET: u16 = 0x21;
    pub const I: u16 = 0x22;
    pub const P: u16 = 0x23;
    pub const L: u16 = 0x25;
    pub const J: u16 = 0x26;
    pub const QUOTE: u16 = 0x27;
    pub const K: u16 = 0x28;
    pub const SEMICOLON: u16 = 0x29;
    pub const BACKSLASH: u16 = 0x2A;
    pub const COMMA: u16 = 0x2B;
    pub const SLASH: u16 = 0x2C;
    pub const N: u16 = 0x2D;
    pub const M: u16 = 0x2E;
    pub const PERIOD: u16 = 0x2F;
    pub const GRAVE: u16 = 0x32;
}

fn char_to_vk(c: char) -> Option<u16> {
    match c {
        'a' | 'A' => Some(ansi::A),
        'b' | 'B' => Some(ansi::B),
        'c' | 'C' => Some(ansi::C),
        'd' | 'D' => Some(ansi::D),
        'e' | 'E' => Some(ansi::E),
        'f' | 'F' => Some(ansi::F),
        'g' | 'G' => Some(ansi::G),
        'h' | 'H' => Some(ansi::H),
        'i' | 'I' => Some(ansi::I),
        'j' | 'J' => Some(ansi::J),
        'k' | 'K' => Some(ansi::K),
        'l' | 'L' => Some(ansi::L),
        'm' | 'M' => Some(ansi::M),
        'n' | 'N' => Some(ansi::N),
        'o' | 'O' => Some(ansi::O),
        'p' | 'P' => Some(ansi::P),
        'q' | 'Q' => Some(ansi::Q),
        'r' | 'R' => Some(ansi::R),
        's' | 'S' => Some(ansi::S),
        't' | 'T' => Some(ansi::T),
        'u' | 'U' => Some(ansi::U),
        'v' | 'V' => Some(ansi::V),
        'w' | 'W' => Some(ansi::W),
        'x' | 'X' => Some(ansi::X),
        'y' | 'Y' => Some(ansi::Y),
        'z' | 'Z' => Some(ansi::Z),
        '0' => Some(ansi::KEY_0),
        '1' => Some(ansi::KEY_1),
        '2' => Some(ansi::KEY_2),
        '3' => Some(ansi::KEY_3),
        '4' => Some(ansi::KEY_4),
        '5' => Some(ansi::KEY_5),
        '6' => Some(ansi::KEY_6),
        '7' => Some(ansi::KEY_7),
        '8' => Some(ansi::KEY_8),
        '9' => Some(ansi::KEY_9),
        '-' | '_' => Some(ansi::MINUS),
        '=' | '+' => Some(ansi::EQUAL),
        '[' | '{' => Some(ansi::LEFT_BRACKET),
        ']' | '}' => Some(ansi::RIGHT_BRACKET),
        '\\' | '|' => Some(ansi::BACKSLASH),
        ';' | ':' => Some(ansi::SEMICOLON),
        '\'' | '"' => Some(ansi::QUOTE),
        '`' | '~' => Some(ansi::GRAVE),
        ',' | '<' => Some(ansi::COMMA),
        '.' | '>' => Some(ansi::PERIOD),
        '/' | '?' => Some(ansi::SLASH),
        ' ' => Some(Vk::SPACE),
        _ => None,
    }
}

impl InputController for MacInputController {
    #[instrument(skip(self), level = "debug")]
    fn mouse_move(&self, x: i32, y: i32, relative: bool) -> alldesk_core::Result<()> {
        let point = {
            let mut pos = self.cursor_pos.lock().unwrap();
            if relative {
                pos.x += x as f64;
                pos.y += y as f64;
            } else {
                pos.x = x as f64;
                pos.y = y as f64;
            }
            CGPoint::new(pos.x, pos.y)
        };

        let event = CGEvent::new_mouse_event(
            self.source.clone(),
            CGEventType::MouseMoved,
            point,
            CGMouseButton::Left,
        )
        .map_err(|_| Error::Input("Failed to create mouse move event".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn mouse_click(&self, button: MouseButton, state: ButtonState) -> alldesk_core::Result<()> {
        let (event_type, cg_button) = match (button, state) {
            (MouseButton::Left, ButtonState::Pressed) => {
                (CGEventType::LeftMouseDown, CGMouseButton::Left)
            }
            (MouseButton::Left, ButtonState::Released) => {
                (CGEventType::LeftMouseUp, CGMouseButton::Left)
            }
            (MouseButton::Right, ButtonState::Pressed) => {
                (CGEventType::RightMouseDown, CGMouseButton::Right)
            }
            (MouseButton::Right, ButtonState::Released) => {
                (CGEventType::RightMouseUp, CGMouseButton::Right)
            }
            (MouseButton::Middle, ButtonState::Pressed) => {
                (CGEventType::OtherMouseDown, CGMouseButton::Center)
            }
            (MouseButton::Middle, ButtonState::Released) => {
                (CGEventType::OtherMouseUp, CGMouseButton::Center)
            }
        };

        let (px, py) = {
            let pos = self.cursor_pos.lock().unwrap();
            (pos.x, pos.y)
        };

        let event = CGEvent::new_mouse_event(
            self.source.clone(),
            event_type,
            CGPoint::new(px, py),
            cg_button,
        )
        .map_err(|_| Error::Input("Failed to create mouse click event".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn mouse_scroll(&self, delta_x: i32, delta_y: i32) -> alldesk_core::Result<()> {
        if delta_x == 0 && delta_y == 0 {
            return Ok(());
        }

        let event = CGEvent::new_scroll_event(
            self.source.clone(),
            ScrollEventUnit::PIXEL,
            2,
            delta_y,
            delta_x,
            0,
        )
        .map_err(|_| Error::Input("Failed to create scroll event".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn key_event(&self, key: KeyCode, state: KeyState) -> alldesk_core::Result<()> {
        let key_down = matches!(state, KeyState::Pressed);

        if let Some(vk) = key_code_to_vk(&key) {
            let event = CGEvent::new_keyboard_event(self.source.clone(), vk, key_down)
                .map_err(|_| Error::Input("Failed to create keyboard event".into()))?;
            event.post(CGEventTapLocation::HID);
        } else {
            match &key {
                KeyCode::Char(c) => {
                    let event = CGEvent::new_keyboard_event(self.source.clone(), 0, key_down)
                        .map_err(|_| {
                            Error::Input("Failed to create Unicode keyboard event".into())
                        })?;
                    event.set_string(&c.to_string());
                    event.post(CGEventTapLocation::HID);
                }
                KeyCode::Unknown(vk_code) => {
                    let event =
                        CGEvent::new_keyboard_event(self.source.clone(), *vk_code as u16, key_down)
                            .map_err(|_| {
                                Error::Input(
                                    "Failed to create keyboard event for unknown VK".into(),
                                )
                            })?;
                    event.post(CGEventTapLocation::HID);
                }
                other => {
                    warn!("Cannot map key code {:?} to macOS virtual key", other);
                    return Err(Error::Input(format!(
                        "Cannot map key code {:?} to macOS virtual key",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    fn unicode_char(&self, ch: char) -> alldesk_core::Result<()> {
        let event = CGEvent::new_keyboard_event(self.source.clone(), 0, true)
            .map_err(|_| Error::Input("Failed to create Unicode char down event".into()))?;
        event.set_string(&ch.to_string());
        event.post(CGEventTapLocation::HID);

        let event = CGEvent::new_keyboard_event(self.source.clone(), 0, false)
            .map_err(|_| Error::Input("Failed to create Unicode char up event".into()))?;
        event.set_string(&ch.to_string());
        event.post(CGEventTapLocation::HID);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Enter), Some(Vk::RETURN));
        assert_eq!(key_code_to_vk(&KeyCode::Escape), Some(Vk::ESCAPE));
        assert_eq!(key_code_to_vk(&KeyCode::Tab), Some(Vk::TAB));
        assert_eq!(key_code_to_vk(&KeyCode::Backspace), Some(Vk::DELETE));
        assert_eq!(key_code_to_vk(&KeyCode::Delete), Some(Vk::FORWARD_DELETE));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowUp), Some(Vk::UP_ARROW));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowDown), Some(Vk::DOWN_ARROW));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowLeft), Some(Vk::LEFT_ARROW));
        assert_eq!(key_code_to_vk(&KeyCode::ArrowRight), Some(Vk::RIGHT_ARROW));
    }

    #[test]
    fn test_function_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Function(1)), Some(Vk::F1));
        assert_eq!(key_code_to_vk(&KeyCode::Function(12)), Some(Vk::F12));
        assert_eq!(key_code_to_vk(&KeyCode::Function(20)), Some(Vk::F20));
        assert_eq!(key_code_to_vk(&KeyCode::Function(0)), None);
        assert_eq!(key_code_to_vk(&KeyCode::Function(21)), None);
    }

    #[test]
    fn test_ascii_char_mapping() {
        assert_eq!(char_to_vk('a'), Some(ansi::A));
        assert_eq!(char_to_vk('Z'), Some(ansi::Z));
        assert_eq!(char_to_vk('0'), Some(ansi::KEY_0));
        assert_eq!(char_to_vk('9'), Some(ansi::KEY_9));
        assert_eq!(char_to_vk(' '), Some(Vk::SPACE));
    }

    #[test]
    fn test_non_ascii_returns_none() {
        assert_eq!(char_to_vk('\u{4e2d}'), None); // CJK '中'
        assert_eq!(char_to_vk('\u{00e9}'), None); // é
    }

    #[test]
    fn test_punctuation_mapping() {
        assert_eq!(char_to_vk('-'), Some(ansi::MINUS));
        assert_eq!(char_to_vk('='), Some(ansi::EQUAL));
        assert_eq!(char_to_vk('['), Some(ansi::LEFT_BRACKET));
        assert_eq!(char_to_vk(']'), Some(ansi::RIGHT_BRACKET));
        assert_eq!(char_to_vk('\\'), Some(ansi::BACKSLASH));
        assert_eq!(char_to_vk(';'), Some(ansi::SEMICOLON));
        assert_eq!(char_to_vk('\''), Some(ansi::QUOTE));
        assert_eq!(char_to_vk('`'), Some(ansi::GRAVE));
        assert_eq!(char_to_vk(','), Some(ansi::COMMA));
        assert_eq!(char_to_vk('.'), Some(ansi::PERIOD));
        assert_eq!(char_to_vk('/'), Some(ansi::SLASH));
    }

    #[test]
    fn test_shift_variant_mapping() {
        assert_eq!(char_to_vk('_'), Some(ansi::MINUS));
        assert_eq!(char_to_vk('+'), Some(ansi::EQUAL));
        assert_eq!(char_to_vk('{'), Some(ansi::LEFT_BRACKET));
        assert_eq!(char_to_vk('}'), Some(ansi::RIGHT_BRACKET));
        assert_eq!(char_to_vk('|'), Some(ansi::BACKSLASH));
        assert_eq!(char_to_vk(':'), Some(ansi::SEMICOLON));
        assert_eq!(char_to_vk('"'), Some(ansi::QUOTE));
        assert_eq!(char_to_vk('~'), Some(ansi::GRAVE));
        assert_eq!(char_to_vk('<'), Some(ansi::COMMA));
        assert_eq!(char_to_vk('>'), Some(ansi::PERIOD));
        assert_eq!(char_to_vk('?'), Some(ansi::SLASH));
    }

    #[test]
    fn test_unknown_key_mapping() {
        assert_eq!(key_code_to_vk(&KeyCode::Unknown(42)), None);
    }
}
