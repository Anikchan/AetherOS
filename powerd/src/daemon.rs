//! Power daemon wiring (P5-T09..T14): telemetry → policy → mechanisms.
//!
//! The policy is pure; this module owns the loop, the persisted stores and the
//! status surface consumed by the control center and Activity Manager.

use std::io;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analytics::{BatteryAnalytics, BatterySample};
use crate::charge::SysfsChargeLimiter;
use crate::estimator::{EstimatorConfig, RateEstimator};
use crate::policy::{LowBatteryState, PowerPolicyConfig, PowerPolicyImpl};
use crate::screentime::ScreenTimeStore;
use crate::tuner::{
    default_rules, DeviceErrorOutcome, SysfsBackend, Tuner, TunerReport, TunerRule,
};
use crate::types::{
    BatteryStatus, BatteryTelemetry, ChargeLimiter, Estimator, LowBatteryAction, PowerAction,
    PowerEvent, PowerPolicy, PowerProfile, PowerSource, ProfileBackend, SleepController,
};

/// Runtime configuration.
#[derive(Debug, Clone)]
pub struct PowerdConfig {
    pub state_dir: PathBuf,
    pub dry_run: bool,
    pub sample_interval_secs: u64,
    pub policy: PowerPolicyConfig,
    pub exclusions_path: PathBuf,
    pub battery_health_path: PathBuf,
    pub screen_time_path: PathBuf,
    pub background_policy_path: PathBuf,
    pub charge_limit: Option<u8>,
}

impl Default for PowerdConfig {
    fn default() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .unwrap_or_else(|| PathBuf::from("/var/lib"))
            .join("aether/powerd");
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
            })
            .unwrap_or_else(|| PathBuf::from("/etc"))
            .join("aether/power");
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/1000"))
            .join("aether");
        Self {
            state_dir: state.clone(),
            dry_run: false,
            sample_interval_secs: 15,
            policy: PowerPolicyConfig::default(),
            exclusions_path: config_dir.join("exclusions.toml"),
            battery_health_path: state.join("battery-history.json"),
            screen_time_path: state.join("screen-time.json"),
            background_policy_path: runtime.join("background-policy.json"),
            charge_limit: None,
        }
    }
}

/// Mechanism bundle, so tests can substitute fakes.
pub struct PowerMechanisms {
    pub telemetry: Box<dyn BatteryTelemetry>,
    pub profiles: Box<dyn ProfileBackend>,
    pub sleep: Box<dyn SleepController>,
    pub background: Box<dyn crate::types::BackgroundPolicySink>,
    pub charge: Box<dyn ChargeLimiter>,
}

/// Status shown by the control center (P5-T14 view, F7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerStatus {
    pub percent: f32,
    pub source: PowerSource,
    pub charging: bool,
    pub profile: PowerProfile,
    pub time_to_empty_secs: Option<u64>,
    pub time_to_full_secs: Option<u64>,
    pub health_percent: Option<f32>,
    pub cycle_count: Option<u32>,
    pub temperature_c: Option<f32>,
    pub low_state: LowBatteryState,
    pub estimating: bool,
    pub charge_limit: Option<u8>,
    pub tuned_devices: Vec<String>,
    pub exclusions: Vec<String>,
    pub tuning_disabled: Option<String>,
    pub battery_reported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerLogEntry {
    pub epoch_secs: u64,
    pub text: String,
}

pub struct PowerDaemon {
    config: PowerdConfig,
    policy: PowerPolicyImpl,
    estimator: RateEstimator,
    tuner: Tuner,
    telemetry: Box<dyn BatteryTelemetry>,
    profiles: Box<dyn ProfileBackend>,
    sleep: Box<dyn SleepController>,
    background: Box<dyn crate::types::BackgroundPolicySink>,
    charge: Box<dyn ChargeLimiter>,
    analytics: BatteryAnalytics,
    screentime: ScreenTimeStore,
    last_status: Option<BatteryStatus>,
    last_profile: Option<PowerProfile>,
    log: Vec<PowerLogEntry>,
    persisted_at: u64,
}

impl PowerDaemon {
    pub fn new(
        config: PowerdConfig,
        mechanisms: PowerMechanisms,
        rules: Option<Vec<TunerRule>>,
    ) -> Self {
        let exclusions = Tuner::load_exclusions(&config.exclusions_path).unwrap_or_default();
        let analytics = BatteryAnalytics::load(&config.battery_health_path)
            .unwrap_or_else(|_| BatteryAnalytics::new(30 * 24 * 60));
        let screentime = ScreenTimeStore::load(&config.screen_time_path)
            .unwrap_or_else(|_| ScreenTimeStore::new());
        let backend = if config.dry_run {
            SysfsBackend::dry_run()
        } else {
            SysfsBackend::system()
        };
        let mut policy = PowerPolicyImpl::new(config.policy);
        policy.set_charge_limit(config.charge_limit);
        Self {
            config,
            policy,
            estimator: RateEstimator::new(EstimatorConfig::default()),
            tuner: Tuner::new(backend, rules.unwrap_or_else(default_rules), exclusions),
            telemetry: mechanisms.telemetry,
            profiles: mechanisms.profiles,
            sleep: mechanisms.sleep,
            background: mechanisms.background,
            charge: mechanisms.charge,
            analytics,
            screentime,
            last_status: None,
            last_profile: None,
            log: Vec::new(),
            persisted_at: 0,
        }
    }

    pub fn status(&self) -> PowerStatus {
        let status = self.last_status.clone().unwrap_or_default();
        PowerStatus {
            percent: status.percent,
            source: status.source,
            charging: status.charging,
            profile: self.policy.profile(),
            time_to_empty_secs: self
                .estimator
                .time_to_empty()
                .map(|duration| duration.as_secs()),
            time_to_full_secs: status.time_to_full.map(|d| d.as_secs()),
            health_percent: status.health_percent,
            cycle_count: status.cycle_count,
            temperature_c: status.temperature_c,
            low_state: self.policy.low_state(),
            estimating: self.estimator.time_to_empty().is_none(),
            charge_limit: self.charge.limit().ok().flatten(),
            tuned_devices: self.tuner.applied_devices(),
            exclusions: self.tuner.exclusions().iter().cloned().collect(),
            tuning_disabled: self.tuner.disabled().map(|s| s.to_string()),
            battery_reported: self.last_status.is_some(),
        }
    }

    /// One telemetry sample + policy evaluation.
    pub fn tick(&mut self, now: Instant, epoch: u64, screen_on: bool) -> Vec<PowerAction> {
        let status = match self.telemetry.read() {
            Ok(status) => status,
            Err(error) => {
                self.push_log(format!("battery telemetry unavailable: {error}"));
                return Vec::new();
            }
        };

        self.estimator.update(&status, now);
        let mut status = status;
        // Prefer the smoothed estimate (D5 section 6).
        if let Some(estimate) = self.estimator.time_to_empty() {
            status.time_to_empty = Some(estimate);
        }

        let mut actions = Vec::new();
        if let Some(previous) = &self.last_status {
            if previous.source != status.source {
                actions.extend(
                    self.policy
                        .on_event(PowerEvent::SourceChanged(status.source)),
                );
            }
        } else {
            actions.extend(
                self.policy
                    .on_event(PowerEvent::SourceChanged(status.source)),
            );
        }
        actions.extend(self.policy.on_event(PowerEvent::BatteryUpdated(status.clone())));

        let sample = BatterySample::from_status(&status, epoch);
        self.analytics.observe(sample, screen_on);

        for action in &actions {
            self.apply(action.clone(), epoch);
        }
        self.last_status = Some(status);

        if epoch.saturating_sub(self.persisted_at) >= 300 {
            self.persist(epoch);
        }
        actions
    }

    /// Apply a profile request from settings.
    pub fn request_profile(&mut self, profile: PowerProfile, epoch: u64) -> Vec<PowerAction> {
        let actions = self.policy.on_event(PowerEvent::UserProfileRequest(profile));
        for action in &actions {
            self.apply(action.clone(), epoch);
        }
        actions
    }

    pub fn set_charge_limit(&mut self, percent: u8, epoch: u64) -> Vec<PowerAction> {
        let actions = self
            .policy
            .on_event(PowerEvent::ChargeLimitRequest(percent));
        for action in &actions {
            self.apply(action.clone(), epoch);
        }
        actions
    }

    pub fn set_lid_closed(&mut self, closed: bool, epoch: u64) -> Vec<PowerAction> {
        let actions = self.policy.on_event(PowerEvent::LidChanged { closed });
        for action in &actions {
            self.apply(action.clone(), epoch);
        }
        actions
    }

    pub fn set_thermal(&mut self, level: crate::types::ThermalLevel, epoch: u64) -> Vec<PowerAction> {
        let actions = self.policy.on_event(PowerEvent::ThermalChanged(level));
        for action in &actions {
            self.apply(action.clone(), epoch);
        }
        actions
    }

    /// Tuner error handling (P5-T10).
    pub fn on_device_error(
        &mut self,
        device: &str,
        now: Instant,
        epoch: u64,
    ) -> DeviceErrorOutcome {
        let outcome = self
            .tuner
            .handle_device_error(device, now, &self.config.exclusions_path);
        self.push_log(format!("device error {device}: {outcome:?}"));
        if epoch > 0 {
            self.persist(epoch);
        }
        outcome
    }

    /// Tune devices when the profile changes (Saver enables the safe list).
    fn apply_tuning_for(&mut self, profile: PowerProfile) {
        if self.last_profile == Some(profile) {
            return;
        }
        let report: TunerReport = self.tuner.set_enabled(profile == PowerProfile::Saver);
        for device in &report.applied {
            self.push_log(format!("tuned {device}"));
        }
        for (device, error) in &report.errors {
            self.push_log(format!("tuning {device} failed: {error}"));
        }
        self.last_profile = Some(profile);
    }

    fn apply(&mut self, action: PowerAction, epoch: u64) {
        match action {
            PowerAction::SetProfile(profile) => {
                if let Err(error) = self.profiles.apply(profile) {
                    self.push_log(format!("profile apply failed: {error}"));
                }
                self.apply_tuning_for(profile);
            }
            PowerAction::SetDevicePowerSaving { device, enabled } => {
                let _ = self.tuner.set_enabled(enabled);
                self.push_log(format!("device {device} power saving={enabled}"));
            }
            PowerAction::SetChargeLimit(percent) => {
                if let Err(error) = self.charge.set_limit(percent) {
                    self.push_log(format!("charge limit failed: {error}"));
                }
            }
            PowerAction::SetBackgroundPolicy(policy) => {
                if let Err(error) = self.background.set(policy) {
                    self.push_log(format!("background policy failed: {error}"));
                }
            }
            PowerAction::SetLowBatteryAction(action) => {
                self.policy.set_low_action(action.clone());
                self.push_log(format!("low battery action set to {action:?}"));
            }
            PowerAction::Notify(message) => self.push_log(format!("notify: {message}")),
            PowerAction::Suspend => {
                if let Err(error) = self.sleep.suspend() {
                    self.push_log(format!("suspend failed: {error}"));
                } else {
                    self.push_log("suspend requested".into());
                }
            }
            PowerAction::Hibernate => {
                if let Err(error) = self.sleep.hibernate() {
                    self.push_log(format!("hibernate failed: {error}"));
                } else {
                    self.push_log("hibernate requested".into());
                }
            }
            PowerAction::Log(text) => self.push_log(text),
        }
        let _ = epoch;
    }

    fn push_log(&mut self, text: String) {
        self.log.push(PowerLogEntry {
            epoch_secs: epoch_secs(),
            text,
        });
        if self.log.len() > 1000 {
            self.log.remove(0);
        }
    }

    pub fn log(&self) -> &[PowerLogEntry] {
        &self.log
    }

    pub fn analytics(&self) -> &BatteryAnalytics {
        &self.analytics
    }

    pub fn screentime(&self) -> &ScreenTimeStore {
        &self.screentime
    }

    /// Screen/focus events from the compositor or shell.
    pub fn on_screen_state(&mut self, on: bool, epoch: u64) {
        self.screentime.set_screen_on(on, epoch);
    }

    pub fn on_focus(&mut self, app: Option<&str>, epoch: u64) {
        self.screentime.set_focus(app, epoch);
    }

    pub fn on_notification(&mut self, app: &str, epoch: u64) {
        self.screentime.record_notification(epoch, app);
    }

    pub fn persist(&mut self, epoch: u64) {
        if self.config.state_dir.as_os_str().is_empty() {
            return;
        }
        let _ = std::fs::create_dir_all(&self.config.state_dir);
        let _ = self.analytics.save(&self.config.battery_health_path);
        self.screentime.flush(epoch);
        let _ = self.screentime.save(&self.config.screen_time_path);
        self.persisted_at = epoch;
    }

    pub fn config(&self) -> &PowerdConfig {
        &self.config
    }

    pub fn policy_config(&self) -> &PowerPolicyConfig {
        &self.config.policy
    }

    pub fn set_low_battery_action(&mut self, action: LowBatteryAction) {
        self.config.policy.low_action = action.clone();
        self.policy.set_low_action(action);
    }
}

pub fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build the system daemon (real UPower/sysfs, power-profiles-daemon, systemd).
pub fn system_daemon(config: PowerdConfig) -> PowerDaemon {
    use crate::backend::{FileBackgroundSink, SystemProfileBackend, SystemdSleep};
    use crate::telemetry::BatteryReader;

    let dry_run = config.dry_run;
    PowerDaemon::new(
        config.clone(),
        PowerMechanisms {
            telemetry: Box::new(BatteryReader::system()),
            profiles: Box::new(SystemProfileBackend::system(dry_run)),
            sleep: if dry_run {
                Box::new(SystemdSleep::dry_run())
            } else {
                Box::new(SystemdSleep::system())
            },
            background: Box::new(FileBackgroundSink::at(
                config.background_policy_path.clone(),
            )),
            charge: Box::new(SysfsChargeLimiter::system()),
        },
        None,
    )
}

/// Persist a status snapshot for the shell (control center reads this file
/// until the D-Bus API lands).
pub fn write_status_file(daemon: &PowerDaemon) -> io::Result<()> {
    let path = daemon.config().state_dir.join("status.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&daemon.status())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BackgroundPolicy, BackgroundPolicySink, LowBatteryAction, PowerError};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct ScriptedBattery {
        queue: Arc<Mutex<VecDeque<BatteryStatus>>>,
        last: Arc<Mutex<BatteryStatus>>,
    }

    impl BatteryTelemetry for ScriptedBattery {
        fn read(&self) -> Result<BatteryStatus, PowerError> {
            let mut queue = self.queue.lock().unwrap();
            if let Some(status) = queue.pop_front() {
                *self.last.lock().unwrap() = status.clone();
            }
            Ok(self.last.lock().unwrap().clone())
        }
    }

    #[derive(Default)]
    struct Recorder {
        profiles: Vec<PowerProfile>,
        sleeps: Vec<&'static str>,
        backgrounds: Vec<BackgroundPolicy>,
        limits: Vec<u8>,
    }

    struct FakeProfiles(Arc<Mutex<Recorder>>);
    impl ProfileBackend for FakeProfiles {
        fn apply(&self, profile: PowerProfile) -> Result<(), PowerError> {
            self.0.lock().unwrap().profiles.push(profile);
            Ok(())
        }
        fn current(&self) -> Result<Option<PowerProfile>, PowerError> {
            Ok(self.0.lock().unwrap().profiles.last().copied())
        }
    }

    struct FakeSleep(Arc<Mutex<Recorder>>);
    impl SleepController for FakeSleep {
        fn suspend(&self) -> Result<(), PowerError> {
            self.0.lock().unwrap().sleeps.push("suspend");
            Ok(())
        }
        fn hibernate(&self) -> Result<(), PowerError> {
            self.0.lock().unwrap().sleeps.push("hibernate");
            Ok(())
        }
    }

    struct FakeBackground(Arc<Mutex<Recorder>>);
    impl BackgroundPolicySink for FakeBackground {
        fn set(&self, policy: BackgroundPolicy) -> Result<(), PowerError> {
            self.0.lock().unwrap().backgrounds.push(policy);
            Ok(())
        }
    }

    struct FakeCharge(Arc<Mutex<Recorder>>);
    impl ChargeLimiter for FakeCharge {
        fn supported(&self) -> bool {
            true
        }
        fn set_limit(&self, percent: u8) -> io::Result<()> {
            self.0.lock().unwrap().limits.push(percent);
            Ok(())
        }
        fn limit(&self) -> io::Result<Option<u8>> {
            Ok(self.0.lock().unwrap().limits.last().copied())
        }
    }

    fn harness() -> (PowerDaemon, Arc<Mutex<Recorder>>, Arc<Mutex<VecDeque<BatteryStatus>>>) {
        let dir = std::env::temp_dir().join(format!(
            "aether-powerd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let recorder = Arc::new(Mutex::new(Recorder::default()));
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let last = Arc::new(Mutex::new(BatteryStatus::default()));
        let config = PowerdConfig {
            state_dir: dir,
            dry_run: true,
            sample_interval_secs: 1,
            policy: PowerPolicyConfig::default(),
            exclusions_path: std::env::temp_dir().join(format!(
                "aether-powerd-{}-exclusions.toml",
                std::process::id()
            )),
            battery_health_path: std::env::temp_dir().join(format!(
                "aether-powerd-{}-battery.json",
                std::process::id()
            )),
            screen_time_path: std::env::temp_dir().join(format!(
                "aether-powerd-{}-screen.json",
                std::process::id()
            )),
            background_policy_path: std::env::temp_dir().join(format!(
                "aether-powerd-{}-background.json",
                std::process::id()
            )),
            charge_limit: None,
        };
        let daemon = PowerDaemon::new(
            config,
            PowerMechanisms {
                telemetry: Box::new(ScriptedBattery {
                    queue: queue.clone(),
                    last,
                }),
                profiles: Box::new(FakeProfiles(recorder.clone())),
                sleep: Box::new(FakeSleep(recorder.clone())),
                background: Box::new(FakeBackground(recorder.clone())),
                charge: Box::new(FakeCharge(recorder.clone())),
            },
            Some(Vec::new()),
        );
        (daemon, recorder, queue)
    }

    fn status(percent: f32, source: PowerSource, charging: bool) -> BatteryStatus {
        BatteryStatus {
            percent,
            source,
            charging,
            energy_rate_w: Some(10.0),
            energy_now_wh: Some(40.0),
            energy_full_wh: Some(50.0),
            cycle_count: Some(100),
            health_percent: Some(90.0),
            ..Default::default()
        }
    }

    #[test]
    fn battery_source_switches_profile_and_background_policy() {
        let (mut daemon, recorder, queue) = harness();
        queue
            .lock()
            .unwrap()
            .push_back(status(80.0, PowerSource::Battery, false));
        daemon.tick(Instant::now(), 1000, true);
        let rec = recorder.lock().unwrap();
        assert!(rec.profiles.contains(&PowerProfile::Saver));
        assert!(rec.backgrounds.contains(&BackgroundPolicy::battery()));
    }

    #[test]
    fn low_battery_triggers_suspend() {
        let (mut daemon, recorder, queue) = harness();
        queue
            .lock()
            .unwrap()
            .push_back(status(80.0, PowerSource::Battery, false));
        daemon.tick(Instant::now(), 1000, true);
        queue
            .lock()
            .unwrap()
            .push_back(status(4.0, PowerSource::Battery, false));
        daemon.tick(Instant::now(), 2000, true);
        assert_eq!(recorder.lock().unwrap().sleeps, vec!["suspend"]);
    }

    #[test]
    fn chart_limit_request_applies() {
        let (mut daemon, recorder, _queue) = harness();
        daemon.set_charge_limit(75, 1000);
        assert_eq!(recorder.lock().unwrap().limits, vec![75]);
        let status = daemon.status();
        assert_eq!(status.charge_limit, Some(75));
    }

    #[test]
    fn status_reports_estimating_before_warmup() {
        let (mut daemon, _recorder, queue) = harness();
        queue
            .lock()
            .unwrap()
            .push_back(status(80.0, PowerSource::Battery, false));
        daemon.tick(Instant::now(), 1000, true);
        let status = daemon.status();
        assert!(status.estimating);
        assert_eq!(status.percent, 80.0);
        assert_eq!(status.source, PowerSource::Battery);
        assert_eq!(status.cycle_count, Some(100));
    }

    #[test]
    fn lid_close_requests_suspend_only_on_battery() {
        let (mut daemon, recorder, queue) = harness();
        queue
            .lock()
            .unwrap()
            .push_back(status(80.0, PowerSource::Battery, false));
        daemon.tick(Instant::now(), 1000, true);
        daemon.set_lid_closed(true, 1001);
        assert_eq!(recorder.lock().unwrap().sleeps, vec!["suspend"]);
    }

    #[test]
    fn analytics_records_samples() {
        let (mut daemon, _recorder, queue) = harness();
        queue
            .lock()
            .unwrap()
            .push_back(status(80.0, PowerSource::Battery, false));
        daemon.tick(Instant::now(), 1000, true);
        assert_eq!(daemon.analytics().cycle_count(), Some(100));
        assert!(daemon.analytics().health_percent().is_some());
    }

    #[test]
    fn screen_time_tracks_focus_and_notifications() {
        let (mut daemon, _recorder, _queue) = harness();
        daemon.on_screen_state(true, 1000);
        daemon.on_focus(Some("org.example.Mail"), 1000);
        daemon.on_notification("org.example.Chat", 1200);
        daemon.on_screen_state(false, 1600);
        let day = (1000 / crate::analytics::DAY_SECS) as i64;
        let summary = daemon.screentime().summary(day).unwrap();
        assert_eq!(summary.screen_on_secs, 600);
        assert_eq!(summary.notification_total(), 1);
    }

    #[test]
    fn low_battery_action_setter_is_exposed() {
        let (mut daemon, _recorder, _queue) = harness();
        daemon.set_low_battery_action(LowBatteryAction::Hibernate);
        assert_eq!(daemon.policy_config().low_action, LowBatteryAction::Hibernate);
    }
}
