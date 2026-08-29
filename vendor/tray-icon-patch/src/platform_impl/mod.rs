// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(target_os = "windows")]
#[path = "windows/mod.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "gtk/mod.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos/mod.rs"]
mod platform;

pub(crate) use self::platform::*;

// Tunante patch: this one has to leave the crate. `pub(crate) use *` above
// keeps everything internal, which is right for the platform types but makes
// `set_symbolic_icon` unreachable from an application — and an application is
// the only thing that knows which icon it wants.
#[cfg(target_os = "linux")]
pub use self::platform::set_symbolic_icon;
