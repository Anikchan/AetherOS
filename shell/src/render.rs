//! Headless rendering of every shell surface (P4 evidence).
//!
//! Each surface gets its own `ShellRoot` and window so the software renderer
//! always draws a fresh frame. Output feeds `docs/.../results/screenshots/` and
//! doubles as a visual regression aid in CI.

use std::path::Path;

use crate::config::{Category, SettingsStore};
use crate::model::Overlay;
use crate::services::MockServices;
use crate::{ShellController, ShellRoot};

pub struct SurfaceSpec {
    pub name: &'static str,
    pub overlay: Overlay,
    pub width: u32,
    pub height: u32,
    pub search: Option<&'static str>,
    pub settings_page: Option<Category>,
    pub lock_error: bool,
}

pub fn specs() -> Vec<SurfaceSpec> {
    vec![
        SurfaceSpec {
            name: "shell-desktop",
            overlay: Overlay::None,
            width: 1440,
            height: 900,
            search: None,
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-launcher",
            overlay: Overlay::Launcher,
            width: 1440,
            height: 900,
            search: Some("ca"),
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-control-center",
            overlay: Overlay::ControlCenter,
            width: 1440,
            height: 900,
            search: None,
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-notifications",
            overlay: Overlay::Notifications,
            width: 1440,
            height: 900,
            search: None,
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-overview",
            overlay: Overlay::Overview,
            width: 1440,
            height: 900,
            search: None,
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-settings",
            overlay: Overlay::Settings,
            width: 1440,
            height: 900,
            search: None,
            settings_page: Some(Category::Appearance),
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-lock",
            overlay: Overlay::Lock,
            width: 1440,
            height: 900,
            search: None,
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-menubar",
            overlay: Overlay::None,
            width: 1440,
            height: 96,
            search: None,
            settings_page: None,
            lock_error: false,
        },
        SurfaceSpec {
            name: "shell-dock",
            overlay: Overlay::None,
            width: 720,
            height: 140,
            search: None,
            settings_page: None,
            lock_error: false,
        },
    ]
}

/// Render every surface into `out_dir`. Returns `(name, path)` pairs.
pub fn render_all(out_dir: &Path) -> Result<Vec<(String, String)>, String> {
    aether_ui::render::init()?;
    std::fs::create_dir_all(out_dir)
        .map_err(|error| format!("cannot create {}: {error}", out_dir.display()))?;

    let mut written = Vec::new();
    for spec in specs() {
        let ui = ShellRoot::new().map_err(|error| format!("create {}: {error}", spec.name))?;
        let store = SettingsStore::new(std::env::temp_dir().join("aether-shell-render"));
        let controller = ShellController::with_demo_content(
            ui,
            Box::new(MockServices::with_battery(84, false)),
            store,
        );
        if let Some(query) = spec.search {
            controller.search(query);
        }
        if spec.lock_error {
            controller.open(Overlay::Lock);
            controller.submit_password("wrong-password");
        }
        controller.open(spec.overlay);
        if let Some(category) = spec.settings_page {
            controller.settings_category(category);
        }

        // The menu bar and dock previews crop the composed desktop.
        if spec.name == "shell-menubar" || spec.name == "shell-dock" {
            controller.open(Overlay::None);
        }

        let path = out_dir.join(format!("{}.png", spec.name));
        let frame =
            aether_ui::render::render_png(controller.ui(), spec.width, spec.height, &path)?;
        written.push((
            spec.name.to_string(),
            format!("{} ({}x{})", path.display(), frame.width, frame.height),
        ));
    }
    Ok(written)
}
