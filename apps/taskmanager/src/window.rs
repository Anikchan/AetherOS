//! Slint window binding for the Activity Manager (P5-T07, F13; P5-T15, F7 PWR-12).
//!
//! The window is presentation only: it renders [`ProcessRow`] values produced by
//! the core and reads every Energy-column string from [`EnergyColumnContract`].
//! That makes the published contract the single source of truth for the header,
//! the null label, the level labels, the design tones and the sort direction, so
//! changing the contract changes the window without touching Slint code.
//!
//! Headless rendering (`--render`) uses the same binding as the on-device
//! window, which is how the P5 evidence screenshots are produced.

slint::include_modules!();

use aether_lifecycled::state::Band;

use slint::{ModelRc, VecModel};

use crate::manager::human_bytes;
use crate::model::{EnergyColumnContract, EnergyImpact, ProcessRow, SystemSample};

/// Height/width used for the headless evidence render.
pub const RENDER_WIDTH: u32 = 1080;
pub const RENDER_HEIGHT: u32 = 640;

/// Controller around the Slint component.
pub struct ActivityManagerWindow {
    ui: ActivityManagerApp,
    contract: EnergyColumnContract,
}

impl ActivityManagerWindow {
    /// Build the window with the shipped contract.
    pub fn new() -> Result<Self, slint::PlatformError> {
        let ui = ActivityManagerApp::new()?;
        Ok(Self::with_contract(ui, EnergyImpact::contract()))
    }

    /// Build the window with an explicit contract (tests, theme overrides).
    pub fn with_contract(ui: ActivityManagerApp, contract: EnergyColumnContract) -> Self {
        apply_contract(&ui, &contract);
        Self { ui, contract }
    }

    pub fn ui(&self) -> &ActivityManagerApp {
        &self.ui
    }

    /// The contract this window is bound to.
    pub fn contract(&self) -> &EnergyColumnContract {
        &self.contract
    }

    /// Re-apply a different contract (used when settings change the column).
    pub fn set_contract(&mut self, contract: EnergyColumnContract) {
        apply_contract(&self.ui, &contract);
        self.contract = contract;
    }

    /// Show rows. `system` fills the CPU/memory summary line; `query` filters
    /// process names case-insensitively. Returns the number of visible rows.
    pub fn bind(
        &self,
        rows: &[ProcessRow],
        system: Option<&SystemSample>,
        query: &str,
    ) -> usize {
        let needle = query.trim().to_ascii_lowercase();
        let mut visible: Vec<&ProcessRow> = rows
            .iter()
            .filter(|row| needle.is_empty() || row.name.to_ascii_lowercase().contains(&needle)
                || row.command.to_ascii_lowercase().contains(&needle))
            .collect();
        visible.sort_by(|a, b| {
            let order = b.energy.score().partial_cmp(&a.energy.score());
            match order {
                Some(std::cmp::Ordering::Equal) | None => b
                    .cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                Some(other) => other,
            }
        });
        if !self.contract.descending() {
            visible.reverse();
        }

        let views: Vec<ProcessRowView> = visible
            .iter()
            .map(|row| self.row_view(row))
            .collect();
        let count = views.len();
        self.ui.set_rows(ModelRc::new(VecModel::from(views)));
        self.ui.set_process_count(count as i32);
        if let Some(system) = system {
            self.ui.set_cpu_summary(
                format!(
                    "CPU {:.1}% · {} cores",
                    system.cpu_total_percent,
                    system.per_core_percent.len().max(1)
                )
                .into(),
            );
            self.ui.set_memory_summary(
                format!(
                    "Memory {} used of {}",
                    human_bytes(
                        system.mem_total_bytes.saturating_sub(system.mem_available_bytes)
                    ),
                    human_bytes(system.mem_total_bytes)
                )
                .into(),
            );
        }
        self.ui.set_energy_sort(self.contract.default_sort.clone().into());
        self.ui
            .set_status(format!("{count} of {} processes", rows.len()).into());
        count
    }

    fn row_view(&self, row: &ProcessRow) -> ProcessRowView {
        let level = if self.contract.level_spec(row.energy).is_some() {
            row.energy
        } else {
            EnergyImpact::None
        };
        ProcessRowView {
            pid: row.pid as i32,
            name: row.name.clone().into(),
            state: row.state.to_string().into(),
            cpu: format!("{:.1}%", row.cpu_percent).into(),
            memory: human_bytes(row.memory_bytes).into(),
            band: row
                .band
                .map(band_label)
                .unwrap_or("")
                .into(),
            energy_label: self.contract.label_for(level).into(),
            energy_tone: self.contract.tone_for(level).into(),
            energy_share: (row.energy.score() / EnergyImpact::High.score()) as f32,
        }
    }
}

/// Level labels are contract values; tokens map to colours in the Slint file.
fn apply_contract(ui: &ActivityManagerApp, contract: &EnergyColumnContract) {
    ui.set_energy_header(contract.header.clone().into());
    ui.set_energy_null_label(contract.null_label.clone().into());
    ui.set_energy_tooltip(contract.tooltip.clone().into());
    ui.set_energy_sort(contract.default_sort.clone().into());
}

fn band_label(band: Band) -> &'static str {
    match band {
        Band::System => "System",
        Band::Foreground => "Foreground",
        Band::Visible => "Visible",
        Band::Protected => "Protected",
        Band::Background => "Background",
        Band::Suspended => "Suspended",
        Band::Hibernated => "Hibernated",
        Band::Terminated => "Terminated",
    }
}

/// Rows used by `--render` when the host has no `/proc` (evidence on macOS).
/// They are labelled as sample data in the status line.
pub fn demo_rows() -> Vec<ProcessRow> {
    vec![
        demo_row(4210, "aether-browser", 'R', 62.5, 980 * 1024 * 1024, EnergyImpact::High, Some(Band::Foreground)),
        demo_row(4311, "aether-player", 'S', 18.2, 420 * 1024 * 1024, EnergyImpact::Medium, Some(Band::Visible)),
        demo_row(4088, "aether-files", 'S', 3.4, 180 * 1024 * 1024, EnergyImpact::Low, None),
        demo_row(1220, "aether-lifecycled", 'S', 1.1, 42 * 1024 * 1024, EnergyImpact::Low, Some(Band::System)),
        demo_row(4902, "aether-bench", 'S', 0.0, 12 * 1024 * 1024, EnergyImpact::None, Some(Band::Background)),
    ]
}

fn demo_row(
    pid: u32,
    name: &str,
    state: char,
    cpu_percent: f32,
    memory_bytes: u64,
    energy: EnergyImpact,
    band: Option<Band>,
) -> ProcessRow {
    ProcessRow {
        pid,
        ppid: 1,
        name: name.into(),
        command: format!("/usr/bin/{name}"),
        user: Some(1000),
        threads: 4,
        open_files: 32,
        state,
        cpu_percent,
        memory_bytes,
        disk_read_bytes: 0,
        disk_write_bytes: 0,
        network_tx_bytes: 0,
        network_rx_bytes: 0,
        gpu_percent: None,
        energy,
        wakeups: 12,
        efficiency: false,
        app_state: None,
        band,
        cgroup: Some(format!("aether-{name}")),
    }
}
