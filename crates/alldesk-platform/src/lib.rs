//! Local platform integration: audio capture/playback, clipboard access and
//! mouse/keyboard injection. Merges the former alldesk-audio,
//! alldesk-clipboard and alldesk-input crates.

pub mod audio {
    mod capture;
    mod playback;

    pub use capture::AudioCapturer;
    pub use playback::AudioPlayer;
}

pub mod clipboard {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "android")] {
            mod android;
            pub use android::{ClipboardContent, ClipboardMonitor};
        } else {
            mod monitor;
            pub use monitor::{ClipboardContent, ClipboardMonitor};
        }
    }

    mod sanitize;
    mod sync;

    pub use sanitize::{sanitize_image, sanitize_text, SanitizeResult};
    pub use sync::ClipboardSync;
}

pub mod input {
    mod controller;
    mod permission;

    pub use controller::{ButtonState, InputController, KeyCode, KeyState, MouseButton};
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
}
