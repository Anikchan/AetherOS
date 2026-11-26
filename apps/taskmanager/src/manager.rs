//! Activity Manager operations: end task, efficiency mode, startup items,
//! services, history merge and export (F13 TM-02/03/12/13/17/19).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use aether_lifecycled::daemon::MetricsSnapshot;
use aether_lifecycled::state::{AppState, Band};
use aether_powerd::analytics::BatteryAnalytics;
use aether_powerd::daemon::PowerStatus;

use crate::collect::{ProcFs, ProcessSampler, SystemSampler};
use crate::model::{
    BatteryView, ExportFormat, PerformanceHistory, ProcessRow, ServiceRow, StartupItem,
    StartupScope, SystemSample, Tab, UserSession,
};

/// One refresh of the window.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub rows: Vec<ProcessRow>,
    pub system: SystemSample,
    pub history: PerformanceHistory,
    pub at_epoch_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndTaskOutcome {
    Signalled { pid: u32, force: bool },
    DryRun { pid: u32, force: bool },
    Refused { pid: u32, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EfficiencyRecord {
    old_nice: i32,
    applied: bool,
}

/// Stateful Activity Manager core.
pub struct ActivityManager {
    fs: ProcFs,
    sampler: ProcessSampler,
    system: SystemSampler,
    efficiency: BTreeMap<u32, EfficiencyRecord>,
    history: PerformanceHistory,
    pub dry_run: bool,
    pub android_pack: bool,
}

impl ActivityManager {
    pub fn new(fs: ProcFs, dry_run: bool) -> Self {
        let sampler = ProcessSampler::new(fs.clone());
        Self {
            fs,
            sampler,
            system: SystemSampler::default(),
            efficiency: BTreeMap::new(),
            history: PerformanceHistory::default(),
            dry_run,
            android_pack: false,
        }
    }

    pub fn tabs(&self) -> Vec<Tab> {
        [
            Tab::Processes,
            Tab::Performance,
            Tab::AppHistory,
            Tab::StartupApps,
            Tab::Details,
            Tab::Services,
            Tab::Battery,
            Tab::Sensors,
            Tab::Users,
            Tab::Android,
        ]
        .into_iter()
        .filter(|tab| tab.visible(self.android_pack))
        .collect()
    }

    pub fn refresh(&mut self, at_epoch_ms: u64) -> io::Result<Snapshot> {
        let rows = self.sampler.sample()?;
        let system = self.system.sample(&self.fs);
        self.history.push(&system, at_epoch_ms);
        Ok(Snapshot {
            rows,
            system,
            history: self.history.clone(),
            at_epoch_ms,
        })
    }

    /// Merge lifecycle states into rows by PID (F2 state / band columns).
    pub fn merge_lifecycle(&self, rows: &mut [ProcessRow], metrics: &MetricsSnapshot) {
        let mut by_pid: BTreeMap<u32, (AppState, Band)> = BTreeMap::new();
        for app in &metrics.rows {
            for pid in &app.pids {
                by_pid.insert(*pid, (app.state, app.band));
            }
        }
        for row in rows.iter_mut() {
            if let Some((state, band)) = by_pid.get(&row.pid) {
                row.app_state = Some(*state);
                row.band = Some(*band);
            }
            if let Some(cgroup) = &row.cgroup {
                // Fall back to the app scope name when pids were not reported.
                if row.app_state.is_none() {
                    if let Some(app) = metrics
                        .rows
                        .iter()
                        .find(|app| cgroup.contains(&format!("aether-app-{}", app.id)))
                    {
                        row.app_state = Some(app.state);
                        row.band = Some(app.band);
                    }
                }
            }
        }
    }

    /// End task (TM-02). Refuses PID 1 and the manager itself.
    pub fn end_task(&mut self, pid: u32, force: bool) -> EndTaskOutcome {
        if pid <= 1 {
            return EndTaskOutcome::Refused {
                pid,
                reason: "PID 1 is the init system".into(),
            };
        }
        if pid == std::process::id() {
            return EndTaskOutcome::Refused {
                pid,
                reason: "refusing to end the Activity Manager itself".into(),
            };
        }
        if self.dry_run {
            return EndTaskOutcome::DryRun { pid, force };
        }
        #[cfg(unix)]
        {
            let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
            let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
            if result == 0 {
                EndTaskOutcome::Signalled { pid, force }
            } else {
                EndTaskOutcome::Refused {
                    pid,
                    reason: io::Error::last_os_error().to_string(),
                }
            }
        }
        #[cfg(not(unix))]
        {
            EndTaskOutcome::Refused {
                pid,
                reason: "signals are not supported on this host".into(),
            }
        }
    }

    /// Efficiency mode (TM-03): lower niceness, and `cpu.weight` when the app
    /// cgroup is known. Disabling restores the previous nice value.
    pub fn set_efficiency(&mut self, pid: u32, enabled: bool) -> io::Result<()> {
        if enabled {
            let old_nice = current_nice(pid).unwrap_or(0);
            if !self.dry_run {
                set_nice(pid, 10)?;
                apply_cpu_weight(pid, &self.fs, 50)?;
            }
            self.efficiency.insert(
                pid,
                EfficiencyRecord {
                    old_nice,
                    applied: true,
                },
            );
            self.sampler.mark_efficiency(pid, true);
        } else {
            if let Some(record) = self.efficiency.remove(&pid) {
                if !self.dry_run && record.applied {
                    let _ = set_nice(pid, record.old_nice);
                    let _ = apply_cpu_weight(pid, &self.fs, 100);
                }
            }
            self.sampler.mark_efficiency(pid, false);
        }
        Ok(())
    }

    pub fn efficiency_pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.efficiency.keys().copied()
    }

    /// Startup items from user and system autostart directories (TM-13).
    pub fn startup_items(&self) -> Vec<StartupItem> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/root"));
        let user = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("autostart");
        let system = PathBuf::from("/etc/xdg/autostart");
        self.startup_items_at(&user, &system)
    }

    /// Testable variant with explicit directories.
    pub fn startup_items_at(&self, user: &Path, system: &Path) -> Vec<StartupItem> {
        let mut items = scan_startup_dir(user, StartupScope::User);
        items.extend(scan_startup_dir(system, StartupScope::System));
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
    }

    /// Enable or disable a startup item persistently (TM-AC-10).
    pub fn set_startup_enabled(&self, desktop_file: &str, enabled: bool) -> io::Result<()> {
        let path = Path::new(desktop_file);
        let text = fs::read_to_string(path)?;
        let updated = set_desktop_hidden(&text, !enabled);
        // Startup edits are explicit user actions, so they are not gated by
        // the signal dry-run flag.
        fs::write(path, updated)
    }

    /// systemd services (TM-17).
    pub fn services(&self) -> Vec<ServiceRow> {
        let output = std::process::Command::new("systemctl")
            .args([
                "list-units",
                "--type=service",
                "--all",
                "--no-legend",
                "--no-pager",
            ])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        parse_systemctl(&String::from_utf8_lossy(&output.stdout))
    }

    /// Logged-in users (TM-14).
    pub fn users(&self) -> Vec<UserSession> {
        let output = std::process::Command::new("loginctl")
            .args(["list-users", "--no-legend"])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        parse_loginctl(&String::from_utf8_lossy(&output.stdout))
    }

    /// Battery tab data from the powerd status file (TM-11).
    pub fn battery_view(&self, power_status_path: &Path) -> Option<BatteryView> {
        let text = fs::read_to_string(power_status_path).ok()?;
        let status: PowerStatus = serde_json::from_str(&text).ok()?;
        Some(BatteryView {
            percent: status.percent,
            charging: status.charging,
            power_w: None,
            time_remaining_secs: status.time_to_empty_secs,
            cycle_count: status.cycle_count,
            health_percent: status.health_percent,
            temperature_c: status.temperature_c,
        })
    }

    /// Energy-impact column data (TM-01/TM-15, F7 PWR-12).
    ///
    /// Ranks the top consumers from the F14 analytics store. Apps not in the
    /// store are `None`; the UI shows "Not reported" for them.
    pub fn apply_energy_impact(&self, rows: &mut [ProcessRow], analytics: &BatteryAnalytics) {
        self.apply_energy_impact_with(
            rows,
            analytics,
            &crate::model::EnergyImpact::contract(),
        );
    }

    /// Same as [`Self::apply_energy_impact`] with an explicit energy contract,
    /// so the Activity Manager window and the classifier share one set of
    /// thresholds (P5-T15).
    pub fn apply_energy_impact_with(
        &self,
        rows: &mut [ProcessRow],
        analytics: &BatteryAnalytics,
        contract: &crate::model::EnergyColumnContract,
    ) {
        let now = analytics.latest_activity_at().unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });
        let totals = analytics.energy_by_app(now, 1);
        let max = totals.first().map(|(_, wh)| *wh).unwrap_or(0.0);
        let mut by_app: BTreeMap<String, f64> = BTreeMap::new();
        for (app, wh) in &totals {
            by_app.insert(app.clone(), *wh);
        }
        for row in rows.iter_mut() {
            let key = row
                .cgroup
                .as_deref()
                .and_then(|cgroup| {
                    app_id_from_scope(cgroup)
                })
                .unwrap_or_else(|| row.name.clone());
            if let Some(wh) = by_app.get(&key) {
                let share = if max > 0.0 { wh / max } else { 0.0 };
                row.energy = contract.level_for(share, *wh);
            }
        }
    }

    /// Export the current rows (TM-19).
    pub fn export(rows: &[ProcessRow], format: ExportFormat) -> String {
        match format {
            ExportFormat::Csv => {
                let mut out = String::from(
                    "pid,ppid,name,state,cpu_percent,memory_bytes,disk_read_bytes,disk_write_bytes,network_rx_bytes,network_tx_bytes,gpu_percent,energy,threads,open_files,wakeups,efficiency,command\n",
                );
                for row in rows {
                    out.push_str(&format!(
                        "{},{},{},{},{:.2},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                        row.pid,
                        row.ppid,
                        csv_escape(&row.name),
                        row.state,
                        row.cpu_percent,
                        row.memory_bytes,
                        row.disk_read_bytes,
                        row.disk_write_bytes,
                        row.network_rx_bytes,
                        row.network_tx_bytes,
                        row.gpu_percent.map(|v| format!("{v:.1}")).unwrap_or_default(),
                        row.energy.label(),
                        row.threads,
                        row.open_files,
                        row.wakeups,
                        row.efficiency,
                        csv_escape(&row.command)
                    ));
                }
                out
            }
            ExportFormat::Json => {
                serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".into())
            }
            ExportFormat::Text => {
                let mut out = String::new();
                out.push_str(&format!(
                    "{:<28} {:>7} {:>8} {:>10} {:>8} {:>6}\n",
                    "Name", "PID", "State", "CPU %", "Memory", "Energy"
                ));
                for row in rows {
                    out.push_str(&format!(
                        "{:<28} {:>7} {:>8} {:>10.1} {:>8} {:>6}\n",
                        truncate(&row.name, 28),
                        row.pid,
                        row.state,
                        row.cpu_percent,
                        human_bytes(row.memory_bytes),
                        row.energy.label()
                    ));
                }
                out.push_str(&format!("\nRows: {}\n", rows.len()));
                out
            }
        }
    }

    /// System table for the Performance tab.
    pub fn performance_text(system: &SystemSample) -> String {
        let used = system
            .mem_total_bytes
            .saturating_sub(system.mem_available_bytes);
        let mut out = String::new();
        out.push_str(&format!("CPU:   {:.1}% total\n", system.cpu_total_percent));
        out.push_str(&format!(
            "Mem:   {} used / {} total\n",
            human_bytes(used),
            human_bytes(system.mem_total_bytes)
        ));
        out.push_str(&format!(
            "Disk:  {:.1} MB/s read, {:.1} MB/s write\n",
            system.disk_read_bytes_per_sec / 1e6,
            system.disk_write_bytes_per_sec / 1e6
        ));
        out.push_str(&format!(
            "Net:   {:.1} KB/s in, {:.1} KB/s out\n",
            system.network_rx_bytes_per_sec / 1e3,
            system.network_tx_bytes_per_sec / 1e3
        ));
        out.push_str(&format!(
            "GPU:   {}\n",
            system
                .gpu_busy_percent
                .map(|v| format!("{v:.0}%"))
                .unwrap_or_else(|| "Not reported".into())
        ));
        out.push_str(&format!(
            "PSI:   {}\n",
            crate::collect::pressure_label(system.pressure_level)
        ));
        out.push_str("Temps: ");
        if system.temperatures_c.is_empty() {
            out.push_str("Not reported\n");
        } else {
            let values: Vec<String> = system
                .temperatures_c
                .iter()
                .map(|(name, value)| format!("{name}={value:.1}°C"))
                .collect();
            out.push_str(&values.join(", "));
            out.push('\n');
        }
        out
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn current_nice(pid: u32) -> io::Result<i32> {
    #[cfg(unix)]
    unsafe {
        // getpriority returns the value - 20 on some systems; errno is not
        // reliable, so normalize into the -20..19 range.
        let value = libc::getpriority(libc::PRIO_PROCESS, pid);
        if value == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(value)
    }
    #[cfg(not(unix))]
    {
        Ok(0)
    }
}

fn set_nice(pid: u32, nice: i32) -> io::Result<()> {
    #[cfg(unix)]
    unsafe {
        if libc::setpriority(libc::PRIO_PROCESS, pid, nice) == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, nice);
        Ok(())
    }
}

fn apply_cpu_weight(pid: u32, fs: &ProcFs, weight: u32) -> io::Result<()> {
    let cgroup = fs::read_to_string(fs.proc_root.join(pid.to_string()).join("cgroup"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.split_once("::")
                    .map(|(_, path)| path.trim().to_string())
            })
        });
    let Some(relative) = cgroup else {
        return Ok(());
    };
    let path = fs.sys_root.join("fs/cgroup").join(relative.trim_start_matches('/'));
    if path.exists() {
        fs::write(path.join("cpu.weight"), weight.to_string())?;
    }
    Ok(())
}

/// Extract the app ID from `.../aether-app-<id>-<hash>.scope`.
pub fn app_id_from_scope(cgroup: &str) -> Option<String> {
    let name = cgroup.rsplit('/').find(|part| part.starts_with("aether-app-"))?;
    let stem = name
        .strip_prefix("aether-app-")?
        .strip_suffix(".scope")?;
    let (id, hash) = stem.rsplit_once('-')?;
    if hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(id.to_string())
    } else {
        None
    }
}

fn scan_startup_dir(dir: &Path, scope: StartupScope) -> Vec<StartupItem> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let name = desktop_value(&text, "Name").unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let exec = desktop_value(&text, "Exec").unwrap_or_default();
        let enabled = !desktop_bool(&text, "Hidden").unwrap_or(false)
            && desktop_bool(&text, "X-GNOME-Autostart-enabled").unwrap_or(true);
        items.push(StartupItem {
            name,
            exec,
            desktop_file: path.to_string_lossy().into_owned(),
            enabled,
            scope,
        });
    }
    items
}

fn desktop_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(|value| value.trim().to_string())
}

fn desktop_bool(text: &str, key: &str) -> Option<bool> {
    desktop_value(text, key).map(|value| match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => true,
        _ => false,
    })
}

/// Set `Hidden=` and `X-GNOME-Autostart-enabled=` without removing other keys.
pub fn set_desktop_hidden(text: &str, hidden: bool) -> String {
    let name_enabled = format!(
        "X-GNOME-Autostart-enabled={}",
        if hidden { "false" } else { "true" }
    );
    let hidden_line = format!("Hidden={}", if hidden { "true" } else { "false" });
    let mut seen_name = false;
    let mut seen_hidden = false;
    let mut lines = Vec::new();
    let mut in_desktop_group = false;
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            in_desktop_group = line.trim() == "[Desktop Entry]";
            lines.push(line.to_string());
            continue;
        }
        if in_desktop_group && line.starts_with("X-GNOME-Autostart-enabled=") {
            lines.push(name_enabled.clone());
            seen_name = true;
            continue;
        }
        if in_desktop_group && line.starts_with("Hidden=") {
            lines.push(hidden_line.clone());
            seen_hidden = true;
            continue;
        }
        lines.push(line.to_string());
        if in_desktop_group && line.trim() == "[Desktop Entry]" {
            // unreachable: group header handled above
        }
    }
    if !seen_name || !seen_hidden {
        let mut insert_at = lines
            .iter()
            .position(|line| line.trim() == "[Desktop Entry]")
            .map(|index| index + 1)
            .unwrap_or(0);
        if !seen_name {
            lines.insert(insert_at, name_enabled);
            insert_at += 1;
            seen_name = true;
        }
        if !seen_hidden {
            lines.insert(insert_at, hidden_line);
            seen_hidden = true;
        }
    }
    let _ = seen_name;
    let _ = seen_hidden;
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn parse_systemctl(text: &str) -> Vec<ServiceRow> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            let unit = parts[0].trim_start_matches(['●', '×', '○']).to_string();
            Some(ServiceRow {
                unit,
                load: parts[1].to_string(),
                active: parts[2].to_string(),
                sub: parts[3].to_string(),
                description: parts.get(4..).map(|rest| rest.join(" ")).unwrap_or_default(),
            })
        })
        .collect()
}

pub fn parse_loginctl(text: &str) -> Vec<UserSession> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                return None;
            }
            let uid = parts[0].parse().ok()?;
            Some(UserSession {
                uid,
                user: parts[1].to_string(),
                seat: parts.get(2).map(|s| s.to_string()),
            })
        })
        .collect()
}

/// IDs referenced by an exported set of rows (used to build the app list).
pub fn app_ids(rows: &[ProcessRow]) -> BTreeSet<String> {
    rows.iter()
        .filter_map(|row| row.cgroup.as_deref().and_then(app_id_from_scope))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-tm-manager-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn row(pid: u32, name: &str) -> ProcessRow {
        ProcessRow {
            pid,
            ppid: 1,
            name: name.into(),
            command: format!("/usr/bin/{name}"),
            user: Some(1000),
            threads: 2,
            open_files: 5,
            state: 'S',
            cpu_percent: 1.0,
            memory_bytes: 1024,
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            network_tx_bytes: 0,
            network_rx_bytes: 0,
            gpu_percent: None,
            energy: crate::model::EnergyImpact::Low,
            wakeups: 0,
            efficiency: false,
            app_state: None,
            band: None,
            cgroup: None,
        }
    }

    #[test]
    fn end_task_refuses_pid_one_and_self() {
        let dir = temp_dir("end");
        let mut manager = ActivityManager::new(ProcFs::at(&dir), true);
        assert!(matches!(
            manager.end_task(1, false),
            EndTaskOutcome::Refused { .. }
        ));
        assert!(matches!(
            manager.end_task(std::process::id(), true),
            EndTaskOutcome::Refused { .. }
        ));
        assert_eq!(
            manager.end_task(4242, false),
            EndTaskOutcome::DryRun {
                pid: 4242,
                force: false
            }
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn efficiency_mode_records_and_restores() {
        let dir = temp_dir("eff");
        let mut manager = ActivityManager::new(ProcFs::at(&dir), true);
        manager.set_efficiency(4242, true).unwrap();
        assert!(manager.efficiency_pids().any(|pid| pid == 4242));
        manager.set_efficiency(4242, false).unwrap();
        assert!(!manager.efficiency_pids().any(|pid| pid == 4242));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn startup_items_are_listed_and_toggled() {
        let dir = temp_dir("startup");
        let autostart = dir.join("autostart");
        fs::create_dir_all(&autostart).unwrap();
        let desktop = autostart.join("aether-mail.desktop");
        let mut file = fs::File::create(&desktop).unwrap();
        writeln!(file, "[Desktop Entry]\nType=Application\nName=Mail\nExec=aether-mail\n").unwrap();

        let manager = ActivityManager::new(ProcFs::at(dir.join("proc")), true);
        let items = manager.startup_items_at(&autostart, &dir.join("system-autostart"));
        let mail = items
            .iter()
            .find(|item| item.name == "Mail")
            .expect("mail startup item");
        assert!(mail.enabled);

        manager
            .set_startup_enabled(&mail.desktop_file, false)
            .unwrap();
        let text = fs::read_to_string(&desktop).unwrap();
        assert!(text.contains("Hidden=true"));
        assert!(text.contains("X-GNOME-Autostart-enabled=false"));

        // Re-scan (real write happens even in dry-run for this action).
        let items = manager.startup_items_at(&autostart, &dir.join("system-autostart"));
        let mail = items.iter().find(|item| item.name == "Mail").unwrap();
        assert!(!mail.enabled);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn export_formats_include_all_rows() {
        let rows = vec![row(10, "browser"), row(11, "text")];
        let csv = ActivityManager::export(&rows, ExportFormat::Csv);
        assert!(csv.starts_with("pid,ppid,name"));
        assert_eq!(csv.lines().count(), 3);
        let json = ActivityManager::export(&rows, ExportFormat::Json);
        assert!(json.contains("\"browser\""));
        let text = ActivityManager::export(&rows, ExportFormat::Text);
        assert!(text.contains("Rows: 2"));
    }

    #[test]
    fn app_id_is_extracted_from_scope() {
        let scope = "/sys/fs/cgroup/aether.slice/aether-apps.slice/aether-app-org.example.Mail-3f2a91c4.scope";
        assert_eq!(app_id_from_scope(scope).as_deref(), Some("org.example.Mail"));
        assert!(app_id_from_scope("/system.slice/sshd.service").is_none());
    }

    #[test]
    fn lifecycle_states_merge_by_pid() {
        use aether_lifecycled::daemon::{AppRow, MetricsSnapshot};
        let metrics = MetricsSnapshot {
            rows: vec![AppRow {
                id: aether_lifecycled::AppId::new("org.example.Mail").unwrap(),
                state: AppState::Suspended,
                band: Band::Suspended,
                memory_current: 42,
                pids: vec![10],
                protect: Vec::new(),
                idle_secs: 5,
            }],
            by_state: BTreeMap::new(),
            total_memory: 42,
            inactive_memory: 42,
            inactive_share: 1.0,
            kills: Vec::<aether_lifecycled::daemon::KillRecord>::new(),
        };
        let dir = temp_dir("merge");
        let manager = ActivityManager::new(ProcFs::at(&dir), true);
        let mut rows = vec![row(10, "mail")];
        manager.merge_lifecycle(&mut rows, &metrics);
        assert_eq!(rows[0].app_state, Some(AppState::Suspended));
        assert_eq!(rows[0].band, Some(Band::Suspended));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn energy_impact_uses_analytics_totals() {
        let dir = temp_dir("energy");
        let manager = ActivityManager::new(ProcFs::at(&dir), true);
        let mut analytics = BatteryAnalytics::new(10);
        analytics.record_app_energy(1000, "org.example.Browser", 18.0);
        analytics.record_app_energy(1000, "org.example.Mail", 2.0);
        let mut rows = vec![row(10, "browser"), row(11, "mail")];
        rows[0].cgroup = Some(
            "/aether.slice/aether-apps.slice/aether-app-org.example.Browser-12345678.scope".into(),
        );
        rows[1].cgroup = Some(
            "/aether.slice/aether-apps.slice/aether-app-org.example.Mail-12345679.scope".into(),
        );
        manager.apply_energy_impact(&mut rows, &analytics);
        assert_eq!(rows[0].energy, crate::model::EnergyImpact::High);
        assert_eq!(rows[1].energy, crate::model::EnergyImpact::Low);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_services_and_users() {
        let services = parse_systemctl(
            "aether-powerd.service loaded active running AetherOS power daemon\n",
        );
        assert_eq!(services[0].unit, "aether-powerd.service");
        assert_eq!(services[0].sub, "running");
        let users = parse_loginctl("1000 anik seat0\n");
        assert_eq!(users[0].uid, 1000);
        assert_eq!(users[0].user, "anik");
    }

    #[test]
    fn desktop_hidden_is_idempotent() {
        let text = "[Desktop Entry]\nType=Application\nName=Mail\n";
        let hidden = set_desktop_hidden(text, true);
        assert!(hidden.contains("Hidden=true"));
        assert_eq!(set_desktop_hidden(&hidden, true), hidden);
        let shown = set_desktop_hidden(&hidden, false);
        assert!(shown.contains("Hidden=false"));
        assert!(shown.contains("X-GNOME-Autostart-enabled=true"));
    }
}
