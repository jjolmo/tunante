// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT
//
// Patched for Tunante: after setting a menu, the first menu item is
// registered as the "secondary activate target" via
// app_indicator_set_secondary_activate_target(). This makes the SNI
// `Activate` D-Bus method (sent on left-click by KDE and other
// compliant desktop environments) trigger that menu item instead of
// opening the context menu.

mod icon;
use std::path::{Path, PathBuf};

use crate::icon::Icon;
pub(crate) use icon::PlatformIcon;

use crate::{TrayIconAttributes, TrayIconId};
use libappindicator::{AppIndicator, AppIndicatorStatus};

pub struct TrayIcon {
    id: TrayIconId,
    indicator: AppIndicator,
    temp_dir_path: Option<PathBuf>,
    path: PathBuf,
    counter: u32,
    menu: Option<Box<dyn muda::ContextMenu>>,
}

impl TrayIcon {
    pub fn new(id: TrayIconId, attrs: TrayIconAttributes) -> crate::Result<Self> {
        let mut indicator = AppIndicator::new(&format!("tray-icon tray app {}", id.as_ref()), "");
        indicator.set_status(AppIndicatorStatus::Active);

        let (parent_path, icon_path) = temp_icon_path(attrs.temp_dir_path.as_ref(), &id, 0)?;

        if let Some(icon) = attrs.icon {
            icon.inner.write_to_png(&icon_path)?;
        }

        indicator.set_icon_theme_path(&parent_path.to_string_lossy());
        indicator.set_icon_full(&icon_path.to_string_lossy(), "icon");

        if let Some(menu) = &attrs.menu {
            indicator.set_menu(&mut menu.gtk_context_menu());
            // Tunante patch: set the first menu item as the secondary
            // activate target so that left-click (Activate D-Bus signal)
            // triggers it instead of opening the full context menu.
            setup_secondary_activate(&indicator, &menu.gtk_context_menu());
        }

        if let Some(title) = attrs.title {
            indicator.set_label(title.as_str(), "");
        }

        Ok(Self {
            id,
            indicator,
            path: icon_path,
            temp_dir_path: attrs.temp_dir_path,
            counter: 0,
            menu: attrs.menu,
        })
    }
    pub fn set_icon(&mut self, icon: Option<Icon>) -> crate::Result<()> {
        let _ = std::fs::remove_file(&self.path);

        self.counter += 1;

        let (parent_path, icon_path) =
            temp_icon_path(self.temp_dir_path.as_ref(), &self.id, self.counter)?;

        if let Some(icon) = icon {
            icon.inner.write_to_png(&icon_path)?;
        }

        self.indicator
            .set_icon_theme_path(&parent_path.to_string_lossy());
        self.indicator
            .set_icon_full(&icon_path.to_string_lossy(), "tray icon");
        self.path = icon_path;

        Ok(())
    }

    pub fn set_menu(&mut self, menu: Option<Box<dyn crate::menu::ContextMenu>>) {
        if let Some(menu) = &menu {
            self.indicator.set_menu(&mut menu.gtk_context_menu());
            // Tunante patch: update secondary activate target when menu changes
            setup_secondary_activate(&self.indicator, &menu.gtk_context_menu());
        }
        self.menu = menu;
    }

    pub fn set_tooltip<S: AsRef<str>>(&mut self, _tooltip: Option<S>) -> crate::Result<()> {
        Ok(())
    }

    pub fn set_title<S: AsRef<str>>(&mut self, title: Option<S>) {
        self.indicator
            .set_label(title.as_ref().map(|t| t.as_ref()).unwrap_or(""), "");
    }

    pub fn set_visible(&mut self, visible: bool) -> crate::Result<()> {
        if visible {
            self.indicator.set_status(AppIndicatorStatus::Active);
        } else {
            self.indicator.set_status(AppIndicatorStatus::Passive);
        }

        Ok(())
    }

    pub fn set_temp_dir_path<P: AsRef<Path>>(&mut self, path: Option<P>) {
        self.temp_dir_path = path.map(|p| p.as_ref().to_path_buf());
    }

    pub fn rect(&self) -> Option<crate::Rect> {
        None
    }

    pub fn app_indicator(&self) -> &AppIndicator {
        &self.indicator
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        self.indicator.set_status(AppIndicatorStatus::Passive);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Sets the first item in the given GTK menu as the AppIndicator's
/// "secondary activate target".
///
/// When a StatusNotifierItem-compliant desktop environment (e.g. KDE
/// Plasma) sends the `Activate` D-Bus method on left-click, libappindicator
/// will emit the "activate" signal on this widget instead of showing the
/// full context menu. This gives us proper left-click → action behaviour
/// on KDE, while GNOME's AppIndicator extension (which always shows the
/// menu) is unaffected.
fn setup_secondary_activate(indicator: &AppIndicator, gtk_menu: &gtk::Menu) {
    use glib::translate::ToGlibPtr;
    use gtk::prelude::ContainerExt;

    let children = gtk_menu.children();
    if let Some(first_child) = children.first() {
        let raw_indicator = indicator.as_raw_ptr();
        // ToGlibPtr gives us *mut gtk_sys::GtkWidget — the same C type
        // that app_indicator_set_secondary_activate_target() expects
        // (libappindicator-sys imports GtkWidget from gtk-sys).
        let widget_ptr: *mut gtk_sys::GtkWidget = first_child.to_glib_none().0;
        unsafe {
            libappindicator_sys::app_indicator_set_secondary_activate_target(
                raw_indicator,
                widget_ptr,
            );
        }
    }
}

/// Generates an icon path in one of the following dirs:
/// 1. If `temp_icon_dir` is `Some` use that.
/// 2. `$XDG_RUNTIME_DIR/tray-icon`
/// 3. `/tmp/tray-icon`
fn temp_icon_path(
    temp_icon_dir: Option<&PathBuf>,
    id: &TrayIconId,
    counter: u32,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let parent_path = match temp_icon_dir.as_ref() {
        Some(path) => path.to_path_buf(),
        None => dirs::runtime_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("tray-icon"),
    };

    std::fs::create_dir_all(&parent_path)?;
    let icon_path = parent_path.join(format!("tray-icon-{}-{}.png", id.as_ref(), counter));
    Ok((parent_path, icon_path))
}

#[test]
fn temp_icon_path_preference_order() {
    let runtime_dir = option_env!("XDG_RUNTIME_DIR");
    let override_dir = PathBuf::from("/tmp/tao-tests");

    let (dir1, _file1) = temp_icon_path(Some(&override_dir), &"00".into(), 00).unwrap();
    let (dir2, _file1) = temp_icon_path(None, &"00".into(), 00).unwrap();
    std::env::remove_var("XDG_RUNTIME_DIR");
    let (dir3, _file2) = temp_icon_path(None, &"00".into(), 00).unwrap();

    assert_eq!(dir1, override_dir);
    if let Some(runtime_dir) = runtime_dir {
        std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
        assert_eq!(dir2, PathBuf::from(format!("{}/tray-icon", runtime_dir)));
    }

    assert_eq!(dir3, PathBuf::from("/tmp/tray-icon"));
}
