pub mod controller;
pub mod permission;

pub use permission::InputPermission;

cfg_if::cfg_if! {
    if #[cfg(target_os = "windows")] {
        mod windows;
        pub use windows::WindowsInputController;
    } else if #[cfg(target_os = "macos")] {
        mod macos;
        pub use macos::MacInputController;
    } else if #[cfg(target_os = "android")] {
        mod android;
        pub use android::AndroidInputController;
    }
}

// On non-Android platforms during testing, include the android module
// so its unit tests can run. It has no platform-specific dependencies.
#[cfg(all(test, not(target_os = "android")))]
mod android;

pub use controller::{InputController, MouseButton, ButtonState, KeyCode, KeyState};
