//! Reference 40-app compatibility spec (P6-T10, 04 section 7, F3).
//!
//! [`HarnessSpec::template`](crate::compat_harness::HarnessSpec::template)
//! produces the 40 slots with placeholder commands. This module adds the
//! *reference* spec the reference machine runs: one representative Windows
//! program per slot, the vendor installer to run through the engine, the
//! installed entry point, a window-title fragment and the documented tweak.
//!
//! The programs are candidates chosen to cover every archetype in 04 §7, not a
//! fixed requirement: swap an entry when a title is unavailable or a better
//! representative exists, as long as the slot name (the database key) stays the
//! same. Installer file names are written without a version pin because
//! installers change often; the payload directory carries the pinned build and
//! `payload-manifest.json` (see the artifact under `apps/runner/compat/`) records
//! the exact version and SHA-256 that was tested.

use std::path::{Path, PathBuf};

use crate::compat::CheckKind;
use crate::compat_harness::{HarnessCheck, HarnessSlot, HarnessSpec};

/// Probe helper contract: `aether-compat-probe <check> [--exe EXE] …` exits 0
/// when the behaviour works and non-zero with a one-line reason when it does
/// not. It is the only command in the spec that is not a stock tool.
pub const PROBE_HELPER: &str = "aether-compat-probe";
/// Where the reference machine keeps the vendor installers.
pub const DEFAULT_PAYLOAD_DIR: &str = "/var/lib/aether/compat/payload";

/// One representative program for a compatibility slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotApp {
    /// Program and version family, for example `Google Chrome`.
    pub app: &'static str,
    /// Vendor, used in the report.
    pub vendor: &'static str,
    /// Vendor installer file name inside the payload directory.
    pub installer: &'static str,
    /// Program the installer creates, relative to the prefix.
    pub exe: &'static str,
    /// Window-title fragment for the window check.
    pub window: &'static str,
    /// Documented prefix tweak (Tier B), if the app needs one.
    pub tweak: Option<&'static str>,
    /// Why this program was chosen for the slot.
    pub notes: &'static str,
}

const fn app(
    app: &'static str,
    vendor: &'static str,
    installer: &'static str,
    exe: &'static str,
    window: &'static str,
    tweak: Option<&'static str>,
    notes: &'static str,
) -> SlotApp {
    SlotApp {
        app,
        vendor,
        installer,
        exe,
        window,
        tweak,
        notes,
    }
}

/// The 40 reference programs, in the same order as
/// [`crate::compat::TEST_SLOTS`].
pub const SLOT_APPS: [SlotApp; 40] = [
    // Browser
    app("Google Chrome", "Google", "ChromeSetup.exe", "drive_c/Program Files/Google/Chrome/Application/chrome.exe", "Google Chrome", None, "Chromium-based browser; Electron apps share this engine"),
    app("Mozilla Firefox", "Mozilla", "Firefox Setup.exe", "drive_c/Program Files/Mozilla Firefox/firefox.exe", "Mozilla Firefox", None, "Gecko browser"),
    app("Brave", "Brave Software", "BraveBrowserSetup.exe", "drive_c/Program Files/BraveSoftware/Brave-Browser/Application/brave.exe", "Brave", None, "Chromium fork with its own updater"),
    // Office and productivity
    app("LibreOffice Writer", "The Document Foundation", "LibreOffice_Win_x86-64.msi", "drive_c/Program Files/LibreOffice/program/soffice.exe", "LibreOffice Writer", None, "Word processor; also covers Calc and Impress"),
    app("LibreOffice Calc", "The Document Foundation", "LibreOffice_Win_x86-64.msi", "drive_c/Program Files/LibreOffice/program/soffice.exe", "LibreOffice Calc", None, "Spreadsheet"),
    app("LibreOffice Impress", "The Document Foundation", "LibreOffice_Win_x86-64.msi", "drive_c/Program Files/LibreOffice/program/soffice.exe", "LibreOffice Impress", None, "Presentation"),
    app("Adobe Acrobat Reader", "Adobe", "AcroRdrDC_en_US.exe", "drive_c/Program Files (x86)/Adobe/Acrobat Reader DC/Reader/AcroRd32.exe", "Adobe Acrobat", None, "PDF reader"),
    app("Notion", "Notion Labs", "Notion-Setup.exe", "drive_c/Users/user/AppData/Local/Programs/Notion/Notion.exe", "Notion", None, "Electron note app"),
    app("Mozilla Thunderbird", "Mozilla", "Thunderbird Setup.exe", "drive_c/Program Files/Mozilla Thunderbird/thunderbird.exe", "Mozilla Thunderbird", None, "Email client"),
    app("Todoist", "Doist", "TodoistSetup.exe", "drive_c/Users/user/AppData/Local/Programs/Todoist/Todoist.exe", "Todoist", None, "Calendar/task manager"),
    app("PDFCreator", "pdfforge", "PDFCreator-Setup.exe", "drive_c/Program Files/PDFCreator/PDFCreator.exe", "PDFCreator", Some("install the PDF printer driver manually; driver installers need admin"), "PDF printer/driver-dependent app"),
    // Media
    app("VLC media player", "VideoLAN", "vlc-win64.exe", "drive_c/Program Files/VideoLAN/VLC/vlc.exe", "VLC media player", None, "Video player; exercises the codec stack"),
    app("foobar2000", "foobar2000", "foobar2000.exe", "drive_c/Program Files (x86)/foobar2000/foobar2000.exe", "foobar2000", None, "Audio player"),
    app("Shotcut", "Meltytech", "shotcut-win64.exe", "drive_c/Program Files/Shotcut/shotcut.exe", "Shotcut", None, "Video editor; needs hardware decode"),
    app("GIMP", "The GIMP Team", "gimp-setup.exe", "drive_c/Program Files/GIMP 2/bin/gimp-2.10.exe", "GIMP", None, "Image editor"),
    app("HandBrake", "HandBrake", "HandBrake-Win_GUI.exe", "drive_c/Program Files/HandBrake/HandBrake.exe", "HandBrake", None, "Media converter"),
    app("Spotify", "Spotify", "SpotifySetup.exe", "drive_c/Users/user/AppData/Roaming/Spotify/Spotify.exe", "Spotify", None, "Streaming client"),
    // Development
    app("Visual Studio Code", "Microsoft", "VSCodeUserSetup-x64.exe", "drive_c/Users/user/AppData/Local/Programs/Microsoft VS Code/Code.exe", "Visual Studio Code", None, "Code editor and Electron baseline"),
    app("Visual Studio 2022 Community", "Microsoft", "vs_community.exe", "drive_c/Program Files/Microsoft Visual Studio/2022/Community/Common7/IDE/devenv.exe", "Microsoft Visual Studio", Some("build an offline layout on Windows first; the web bootstrapper needs MS servers"), "IDE with debugger"),
    app("PuTTY", "PuTTY", "putty-64bit-installer.msi", "drive_c/Program Files/PuTTY/putty.exe", "PuTTY", None, "Terminal tool (SSH/serial)"),
    app("DBeaver", "DBeaver", "dbeaver-ce-setup.exe", "drive_c/Program Files/DBeaver/dbeaver.exe", "DBeaver", None, "Database client; needs a running PostgreSQL for the network check"),
    app("GitHub Desktop", "GitHub", "GitHubDesktopSetup-x64.exe", "drive_c/Users/user/AppData/Local/GitHubDesktop/GitHubDesktop.exe", "GitHub Desktop", None, "Version-control GUI"),
    app("Docker Desktop", "Docker", "Docker Desktop Installer.exe", "drive_c/Program Files/Docker/Docker/Docker Desktop.exe", "Docker Desktop", Some("requires WSL2/Hyper-V; expected Tier D without a Windows kernel"), "Container GUI"),
    app("Postman", "Postman", "Postman-win64-Setup.exe", "drive_c/Users/user/AppData/Local/Postman/Postman.exe", "Postman", None, "API/HTTP client"),
    // Games
    app("Fallout 4", "Bethesda", "setup_fallout_4.exe", "drive_c/GOG Games/Fallout 4/Fallout4.exe", "Fallout4", None, "DirectX 11 game (GOG offline installer)"),
    app("Cyberpunk 2077", "CD Projekt", "setup_cyberpunk_2077.exe", "drive_c/GOG Games/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe", "Cyberpunk 2077", Some("force vkd3d-proton in the prefix"), "DirectX 12 game"),
    app("Celeste", "Maddy Makes Games", "setup_celeste.exe", "drive_c/GOG Games/Celeste/Celeste.exe", "Celeste", None, "Indie game (OpenGL/FNA)"),
    app("Steam", "Valve", "SteamSetup.exe", "drive_c/Program Files (x86)/Steam/steam.exe", "Steam", None, "Launcher-based game library"),
    app("Cuphead", "Studio MDHR", "setup_cuphead.exe", "drive_c/GOG Games/Cuphead/Cuphead.exe", "Cuphead", None, "Unity game"),
    app("ABZU", "Giant Squid", "setup_abzu.exe", "drive_c/GOG Games/ABZU/ABZU.exe", "ABZU", None, "Unreal Engine game"),
    app("Age of Empires II (1999)", "Ensemble Studios", "setup_aoe2.exe", "drive_c/GOG Games/Age of Empires II/Empires2.exe", "Age of Empires II", None, "Older 32-bit game"),
    app("VALORANT", "Riot Games", "RiotClientInstalls.exe", "drive_c/Riot Games/VALORANT/live/VALORANT.exe", "VALORANT", Some("Vanguard anti-cheat needs a Windows kernel; expected Tier D"), "Anti-cheat multiplayer game"),
    // Utilities
    app("7-Zip", "Igor Pavlov", "7z-x64.exe", "drive_c/Program Files/7-Zip/7zFM.exe", "7-Zip", None, "Archive tool"),
    app("Dropbox", "Dropbox", "DropboxInstaller.exe", "drive_c/Program Files (x86)/Dropbox/Dropbox.exe", "Dropbox", None, "File sync client"),
    app("KeePass 2", "Dominik Reichl", "KeePass-Setup.exe", "drive_c/Program Files/KeePass Password Safe 2/KeePass.exe", "KeePass", None, "Password manager"),
    app("HWiNFO64", "Martin Malik", "HWiNFO64-Setup.exe", "drive_c/Program Files/HWiNFO64/HWiNFO64.exe", "HWiNFO", None, "System monitor; sensor access is expected to need a tweak"),
    app("ShareX", "ShareX Team", "ShareX-setup.exe", "drive_c/Program Files/ShareX/ShareX.exe", "ShareX", None, "Screenshot/annotation app"),
    app("AnyDesk", "AnyDesk Software", "AnyDesk.exe", "drive_c/Program Files (x86)/AnyDesk/AnyDesk.exe", "AnyDesk", None, "Remote desktop client"),
    app("WireGuard", "WireGuard", "wireguard-installer.exe", "drive_c/Program Files/WireGuard/wireguard.exe", "WireGuard", None, "VPN client; tunnelling needs the TUN device"),
    app("Duplicati", "Duplicati", "Duplicati.msi", "drive_c/Program Files/Duplicati/Duplicati.GUI.TrayIcon.exe", "Duplicati", None, "Backup utility"),
];

/// Stable app id for a slot, for example `com.aether.compat.browser-app-1`.
pub fn app_id(slot_name: &str) -> String {
    let slug: String = slot_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').replace("--", "-");
    format!("com.aether.compat.{slug}")
}

fn slug(slot_name: &str) -> String {
    app_id(slot_name)
        .rsplit('.')
        .next()
        .unwrap_or("slot")
        .to_string()
}

/// Build one slot with the reference installer and probe commands.
pub fn slot(slot_name: &str, app: &SlotApp, payload_dir: &Path) -> HarnessSlot {
    let id = app_id(slot_name);
    let installer = payload_dir.join(app.installer);
    let exe = app.exe;
    let window = app.window;
    let checks = vec![
        HarnessCheck {
            timeout_secs: 900,
            ..HarnessCheck::new(
                CheckKind::Install,
                "aether-runner",
                vec![
                    "windows".into(),
                    "install".into(),
                    "--installer".into(),
                    installer.display().to_string(),
                    "--id".into(),
                    id.clone(),
                    "--entry".into(),
                    exe.into(),
                ],
            )
        },
        HarnessCheck {
            timeout_secs: 180,
            ..HarnessCheck::new(
                CheckKind::Launch,
                "aether-runner",
                vec!["launch".into(), id.clone()],
            )
        },
        HarnessCheck {
            timeout_secs: 60,
            ..HarnessCheck::new(
                CheckKind::Window,
                "xdotool",
                vec!["search".into(), "--name".into(), window.into()],
            )
        },
        HarnessCheck {
            timeout_secs: 120,
            ..HarnessCheck::new(
                CheckKind::Input,
                PROBE_HELPER,
                vec![
                    "input".into(),
                    "--window".into(),
                    window.into(),
                    "--exe".into(),
                    exe.into(),
                ],
            )
        },
        HarnessCheck::new(
            CheckKind::Audio,
            PROBE_HELPER,
            vec!["audio".into(), "--id".into(), id.clone()],
        ),
        HarnessCheck::new(
            CheckKind::Video,
            PROBE_HELPER,
            vec!["video".into(), "--id".into(), id.clone()],
        ),
        HarnessCheck::new(
            CheckKind::Files,
            PROBE_HELPER,
            vec!["files".into(), "--id".into(), id.clone()],
        ),
        HarnessCheck::new(
            CheckKind::Network,
            PROBE_HELPER,
            vec!["network".into(), "--id".into(), id.clone()],
        ),
        HarnessCheck::new(
            CheckKind::Persistence,
            PROBE_HELPER,
            vec!["persistence".into(), "--id".into(), id.clone()],
        ),
        HarnessCheck {
            timeout_secs: 300,
            ..HarnessCheck::new(
                CheckKind::Stability,
                PROBE_HELPER,
                vec!["stability".into(), "--id".into(), id, "--seconds".into(), "60".into()],
            )
        },
    ];
    HarnessSlot {
        name: slot_name.to_string(),
        installer,
        args: vec![exe.into()],
        tweak: app.tweak.map(str::to_string),
        checks,
        recorded: Vec::new(),
    }
}

/// The full reference spec, in `TEST_SLOTS` order.
pub fn spec(engine: &str, machine: &str, payload_dir: &Path) -> HarnessSpec {
    let slots = crate::compat::TEST_SLOTS
        .iter()
        .enumerate()
        .map(|(index, (category, _))| {
            let number = crate::compat::TEST_SLOTS[..=index]
                .iter()
                .filter(|(name, _)| name == category)
                .count();
            let name = format!("{category} app {number}");
            slot(&name, &SLOT_APPS[index], payload_dir)
        })
        .collect();
    HarnessSpec {
        engine: engine.to_string(),
        machine: machine.to_string(),
        slots,
    }
}

/// Where an app's window title fragment is documented, for the report table.
pub fn window_for(slot_name: &str) -> Option<&'static str> {
    crate::compat::TEST_SLOTS
        .iter()
        .enumerate()
        .find(|(index, (category, _))| {
            let number = crate::compat::TEST_SLOTS[..=*index]
                .iter()
                .filter(|(name, _)| name == category)
                .count();
            format!("{category} app {number}") == slot_name
        })
        .map(|(index, _)| SLOT_APPS[index].window)
}

/// The slot slug used in the app id and the payload manifest.
pub fn slot_slug(slot_name: &str) -> String {
    slug(slot_name)
}

/// Default payload directory as a [`PathBuf`].
pub fn default_payload_dir() -> PathBuf {
    PathBuf::from(DEFAULT_PAYLOAD_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::{CheckKind, CompatDb};

    #[test]
    fn the_table_has_one_representative_program_per_slot() {
        assert_eq!(SLOT_APPS.len(), crate::compat::TEST_SLOTS.len());
        for (index, entry) in SLOT_APPS.iter().enumerate() {
            assert!(!entry.app.is_empty(), "slot {index} has no app");
            assert!(!entry.installer.is_empty(), "slot {index} has no installer");
            assert!(!entry.exe.is_empty(), "slot {index} has no entry point");
            assert!(!entry.window.is_empty(), "slot {index} has no window probe");
            assert!(
                entry.installer.ends_with(".exe") || entry.installer.ends_with(".msi"),
                "slot {index} installer is not a Windows package"
            );
        }
    }

    #[test]
    fn slot_names_and_ids_match_the_database() {
        let spec = spec("wine-10.0", "reference-machine", Path::new("/payload"));
        let db = CompatDb::test_plan();
        assert_eq!(spec.slots.len(), 40);
        for (built, entry) in spec.slots.iter().zip(db.entries.iter()) {
            assert_eq!(&built.name, &entry.name);
            let id = app_id(&built.name);
            assert!(aether_pkg::manifest::AppId::new(id.clone()).is_ok(), "{id}");
            assert!(id.starts_with("com.aether.compat."));
        }
    }

    #[test]
    fn every_slot_has_a_command_for_each_check() {
        let spec = spec("wine-10.0", "dev-host", Path::new("/payload"));
        for slot in &spec.slots {
            let kinds: Vec<CheckKind> = slot.checks.iter().map(|check| check.check).collect();
            for kind in CheckKind::ALL {
                assert!(kinds.contains(&kind), "{} misses {kind:?}", slot.name);
            }
            assert!(!slot.installer.as_os_str().is_empty());
            assert!(slot.installer.starts_with("/payload"));
            assert!(slot.recorded.is_empty());
        }
    }

    #[test]
    fn installer_command_uses_the_payload_and_the_entry_point() {
        let spec = spec("wine-10.0", "dev-host", Path::new("/payload"));
        let first = &spec.slots[0];
        assert_eq!(first.installer, PathBuf::from("/payload/ChromeSetup.exe"));
        let install = first
            .checks
            .iter()
            .find(|check| check.check == CheckKind::Install)
            .unwrap();
        assert_eq!(install.program, "aether-runner");
        assert!(install.args.iter().any(|arg| arg == "/payload/ChromeSetup.exe"));
        assert!(install.args.iter().any(|arg| arg == "com.aether.compat.browser-app-1"));
        assert!(install.args.iter().any(|arg| arg.ends_with("chrome.exe")));
        assert_eq!(install.timeout_secs, 900);
        let launch = first
            .checks
            .iter()
            .find(|check| check.check == CheckKind::Launch)
            .unwrap();
        assert_eq!(launch.args, vec!["launch".to_string(), "com.aether.compat.browser-app-1".into()]);
    }

    #[test]
    fn known_tier_b_and_tier_d_candidates_carry_a_documented_tweak() {
        let spec = spec("wine-10.0", "dev-host", Path::new("/payload"));
        let val = spec
            .slots
            .iter()
            .find(|slot| slot.name == "Games app 8")
            .unwrap();
        assert!(val.tweak.as_deref().unwrap_or("").contains("Vanguard"));
        let dx12 = spec
            .slots
            .iter()
            .find(|slot| slot.name == "Games app 2")
            .unwrap();
        assert!(dx12.tweak.as_deref().unwrap_or("").contains("vkd3d"));
        let plain = spec
            .slots
            .iter()
            .find(|slot| slot.name == "Browser app 1")
            .unwrap();
        assert!(plain.tweak.is_none());
    }

    #[test]
    fn window_lookup_follows_the_slot_name() {
        assert_eq!(window_for("Browser app 1"), Some("Google Chrome"));
        assert_eq!(window_for("Utilities app 6"), Some("AnyDesk"));
        assert_eq!(window_for("Not a slot"), None);
        assert_eq!(slot_slug("Office and productivity app 10"), "office-and-productivity-app-10");
    }
}
