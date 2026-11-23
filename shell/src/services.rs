//! System service adapters used by the shell (P4, S3).
//!
//! The shell never talks to hardware directly: Wi-Fi goes through NetworkManager
//! (`nmcli`), Bluetooth through BlueZ (`bluetoothctl`), audio through PipeWire
//! (`wpctl`), brightness through `brightnessctl`, power profiles through
//! `powerprofilesctl`, and battery state is read from sysfs. Every command is
//! built by a pure function so it can be reviewed and tested without spawning a
//! process. `MockServices` records calls for unit and UI tests.

use std::process::Command;

use crate::model::PowerMode;

/// Battery state read from `/sys/class/power_supply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

pub trait Services {
    fn set_wifi(&mut self, on: bool) -> Result<(), String>;
    fn set_bluetooth(&mut self, on: bool) -> Result<(), String>;
    fn set_volume(&mut self, volume: f32) -> Result<(), String>;
    fn set_brightness(&mut self, brightness: f32) -> Result<(), String>;
    fn set_power_mode(&mut self, mode: PowerMode) -> Result<(), String>;
    fn battery(&mut self) -> Option<Battery>;
    fn lock_session(&mut self) -> Result<(), String>;
}

// ------------------------------------------------------------- command lines ---

pub fn wifi_argv(on: bool) -> Vec<String> {
    vec![
        "nmcli".into(),
        "radio".into(),
        "wifi".into(),
        if on { "on" } else { "off" }.into(),
    ]
}

pub fn bluetooth_argv(on: bool) -> Vec<String> {
    vec![
        "bluetoothctl".into(),
        "power".into(),
        if on { "on" } else { "off" }.into(),
    ]
}

pub fn volume_argv(volume: f32) -> Vec<String> {
    let percent = (volume.clamp(0.0, 1.0) * 100.0).round() as u32;
    vec![
        "wpctl".into(),
        "set-volume".into(),
        "@DEFAULT_AUDIO_SINK@".into(),
        format!("{percent}%"),
    ]
}

pub fn brightness_argv(brightness: f32) -> Vec<String> {
    let percent = (brightness.clamp(0.0, 1.0) * 100.0).round() as u32;
    vec!["brightnessctl".into(), "set".into(), format!("{percent}%")]
}

pub fn power_mode_argv(mode: PowerMode) -> Vec<String> {
    vec![
        "powerprofilesctl".into(),
        "set".into(),
        match mode {
            PowerMode::Saver => "power-saver",
            PowerMode::Balanced => "balanced",
            PowerMode::Performance => "performance",
        }
        .into(),
    ]
}

pub fn parse_capacity(text: &str) -> Option<u8> {
    text.trim().parse::<u8>().ok().filter(|value| *value <= 100)
}

pub fn parse_status(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "charging" | "full"
    )
}

// --------------------------------------------------------------- real adapter ---

/// Runs the system commands. Failures are reported but never fatal: the shell
/// keeps its optimistic UI state and logs through the returned `Err`.
#[derive(Debug, Default)]
pub struct CommandServices {
    pub dry_run: bool,
}

impl CommandServices {
    pub fn new() -> CommandServices {
        CommandServices { dry_run: false }
    }

    fn run(&self, argv: &[String]) -> Result<(), String> {
        if self.dry_run {
            return Ok(());
        }
        let Some((program, args)) = argv.split_first() else {
            return Err("empty command".to_string());
        };
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| format!("{program}: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("{program} failed: {}", stderr.trim()))
        }
    }
}

impl Services for CommandServices {
    fn set_wifi(&mut self, on: bool) -> Result<(), String> {
        self.run(&wifi_argv(on))
    }

    fn set_bluetooth(&mut self, on: bool) -> Result<(), String> {
        self.run(&bluetooth_argv(on))
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.run(&volume_argv(volume))
    }

    fn set_brightness(&mut self, brightness: f32) -> Result<(), String> {
        self.run(&brightness_argv(brightness))
    }

    fn set_power_mode(&mut self, mode: PowerMode) -> Result<(), String> {
        self.run(&power_mode_argv(mode))
    }

    fn battery(&mut self) -> Option<Battery> {
        read_battery()
    }

    fn lock_session(&mut self) -> Result<(), String> {
        self.run(&["loginctl".into(), "lock-session".into()])
    }
}

/// Read the first battery in `/sys/class/power_supply`.
pub fn read_battery() -> Option<Battery> {
    let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        // Some power supplies (mains, USB) have no `type` file; skip those
        // instead of aborting the whole scan.
        let Ok(kind) = std::fs::read_to_string(path.join("type")) else {
            continue;
        };
        if kind.trim() != "Battery" {
            continue;
        }
        let Some(percent) = std::fs::read_to_string(path.join("capacity"))
            .ok()
            .and_then(|text| parse_capacity(&text))
        else {
            continue;
        };
        let charging = std::fs::read_to_string(path.join("status"))
            .map(|text| parse_status(&text))
            .unwrap_or(false);
        return Some(Battery { percent, charging });
    }
    None
}

// ----------------------------------------------------------------- mock ---

/// Mock adapter for tests and for the headless screenshot renderer.
#[derive(Debug, Default)]
pub struct MockServices {
    pub calls: Vec<String>,
    pub battery: Option<Battery>,
}

impl MockServices {
    pub fn with_battery(percent: u8, charging: bool) -> MockServices {
        MockServices {
            calls: Vec::new(),
            battery: Some(Battery { percent, charging }),
        }
    }
}

impl Services for MockServices {
    fn set_wifi(&mut self, on: bool) -> Result<(), String> {
        self.calls.push(format!("wifi {on}"));
        Ok(())
    }

    fn set_bluetooth(&mut self, on: bool) -> Result<(), String> {
        self.calls.push(format!("bluetooth {on}"));
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.calls.push(format!("volume {volume:.2}"));
        Ok(())
    }

    fn set_brightness(&mut self, brightness: f32) -> Result<(), String> {
        self.calls.push(format!("brightness {brightness:.2}"));
        Ok(())
    }

    fn set_power_mode(&mut self, mode: PowerMode) -> Result<(), String> {
        self.calls.push(format!("power {}", mode.as_str()));
        Ok(())
    }

    fn battery(&mut self) -> Option<Battery> {
        self.battery
    }

    fn lock_session(&mut self) -> Result<(), String> {
        self.calls.push("lock".to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_match_the_expected_tools() {
        assert_eq!(wifi_argv(true), ["nmcli", "radio", "wifi", "on"]);
        assert_eq!(wifi_argv(false), ["nmcli", "radio", "wifi", "off"]);
        assert_eq!(bluetooth_argv(true), ["bluetoothctl", "power", "on"]);
        assert_eq!(
            volume_argv(0.625),
            ["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "63%"]
        );
        assert_eq!(brightness_argv(0.8), ["brightnessctl", "set", "80%"]);
        assert_eq!(
            power_mode_argv(PowerMode::Saver),
            ["powerprofilesctl", "set", "power-saver"]
        );
    }

    #[test]
    fn volume_percent_is_clamped() {
        assert_eq!(volume_argv(2.0).last().unwrap(), "100%");
        assert_eq!(volume_argv(-1.0).last().unwrap(), "0%");
    }

    #[test]
    fn battery_parsing_is_defensive() {
        assert_eq!(parse_capacity(" 87\n"), Some(87));
        assert_eq!(parse_capacity("101"), None);
        assert_eq!(parse_capacity("n/a"), None);
        assert!(parse_status("Charging\n"));
        assert!(parse_status("Full"));
        assert!(!parse_status("Discharging"));
    }

    #[test]
    fn mock_records_calls() {
        let mut services = MockServices::with_battery(50, false);
        services.set_wifi(false).unwrap();
        services.set_volume(0.5).unwrap();
        services.set_power_mode(PowerMode::Performance).unwrap();
        assert_eq!(
            services.calls,
            ["wifi false", "volume 0.50", "power performance"]
        );
        assert_eq!(
            services.battery(),
            Some(Battery {
                percent: 50,
                charging: false
            })
        );
    }

    #[test]
    fn dry_run_commands_report_success() {
        let mut services = CommandServices { dry_run: true };
        assert!(services.set_wifi(true).is_ok());
        assert!(services.lock_session().is_ok());
    }
}
