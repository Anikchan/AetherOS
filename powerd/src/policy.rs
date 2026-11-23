//! Power policy rules (P5-T09 and P5-T13, D5 sections 5, 7 and 8).
//!
//! Rule priority: safety (low battery, thermal) > user choice > automatic
//! profile switching. The policy is pure: it takes events, returns actions.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::types::{
    BackgroundPolicy, BatteryStatus, LowBatteryAction, PowerAction, PowerEvent, PowerPolicy,
    PowerProfile, PowerSource, PowerThresholds, ThermalLevel,
};

/// Low-battery states from D5 section 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LowBatteryState {
    Normal,
    Warned20,
    Saver10,
    ActionDue5,
    Critical3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Threshold {
    Warn20,
    Saver10,
    Action5,
    Critical3,
}

impl Threshold {
    pub fn percent(self, thresholds: PowerThresholds) -> f32 {
        match self {
            Self::Warn20 => thresholds.warn,
            Self::Saver10 => thresholds.saver,
            Self::Action5 => thresholds.action,
            Self::Critical3 => thresholds.critical,
        }
    }

    pub fn state(self) -> LowBatteryState {
        match self {
            Self::Warn20 => LowBatteryState::Warned20,
            Self::Saver10 => LowBatteryState::Saver10,
            Self::Action5 => LowBatteryState::ActionDue5,
            Self::Critical3 => LowBatteryState::Critical3,
        }
    }
}

/// What the low-battery machine decided for one sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowBatteryDecision {
    pub threshold: Threshold,
    pub profile: Option<PowerProfile>,
    pub sleep: Option<LowBatteryAction>,
    pub notify: String,
}

/// The D5 section 7 state machine. Each threshold runs once per discharge
/// cycle; plugging in AC resets it.
#[derive(Debug, Clone)]
pub struct LowBatteryMachine {
    thresholds: PowerThresholds,
    state: LowBatteryState,
    fired: HashSet<Threshold>,
    low_action: LowBatteryAction,
    swap_available: bool,
}

impl LowBatteryMachine {
    pub fn new(
        thresholds: PowerThresholds,
        low_action: LowBatteryAction,
        swap_available: bool,
    ) -> Self {
        Self {
            thresholds,
            state: LowBatteryState::Normal,
            fired: HashSet::new(),
            low_action,
            swap_available,
        }
    }

    pub fn state(&self) -> LowBatteryState {
        self.state
    }

    pub fn set_low_action(&mut self, action: LowBatteryAction) {
        self.low_action = action;
    }

    pub fn set_swap_available(&mut self, available: bool) {
        self.swap_available = available;
    }

    pub fn reset(&mut self) {
        self.state = LowBatteryState::Normal;
        self.fired.clear();
    }

    /// Feed one battery sample. Returns decisions for thresholds crossed since
    /// the previous sample, highest first.
    pub fn update(&mut self, percent: f32, charging: bool) -> Vec<LowBatteryDecision> {
        if charging {
            self.reset();
            return Vec::new();
        }
        let mut decisions = Vec::new();
        for threshold in [
            Threshold::Warn20,
            Threshold::Saver10,
            Threshold::Action5,
            Threshold::Critical3,
        ] {
            let limit = threshold.percent(self.thresholds);
            if percent <= limit && !self.fired.contains(&threshold) {
                self.fired.insert(threshold);
                self.state = threshold.state();
                let decision = self.decide(threshold, percent);
                decisions.push(decision);
            }
        }
        decisions
    }

    fn decide(&self, threshold: Threshold, percent: f32) -> LowBatteryDecision {
        match threshold {
            Threshold::Warn20 => LowBatteryDecision {
                threshold,
                profile: None,
                sleep: None,
                notify: format!("Battery at {percent:.0}%. Consider plugging in."),
            },
            Threshold::Saver10 => LowBatteryDecision {
                threshold,
                profile: Some(PowerProfile::Saver),
                sleep: None,
                notify: format!("Battery at {percent:.0}%. Saver profile enabled."),
            },
            Threshold::Action5 => LowBatteryDecision {
                threshold,
                profile: Some(PowerProfile::Saver),
                sleep: Some(self.low_action.clone()),
                notify: format!(
                    "Battery at {percent:.0}%. {} now.",
                    match self.low_action {
                        LowBatteryAction::Suspend => "Suspending",
                        LowBatteryAction::Hibernate => "Hibernating",
                    }
                ),
            },
            Threshold::Critical3 => {
                let action = if self.swap_available {
                    LowBatteryAction::Hibernate
                } else {
                    LowBatteryAction::Suspend
                };
                LowBatteryDecision {
                    threshold,
                    profile: Some(PowerProfile::Saver),
                    sleep: Some(action),
                    notify: format!("Battery critical ({percent:.0}%). Saving your session."),
                }
            }
        }
    }
}

/// Configuration for the whole power policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerPolicyConfig {
    pub thresholds: PowerThresholds,
    pub low_action: LowBatteryAction,
    pub swap_available: bool,
    pub suspend_then_hibernate: bool,
    pub ac_background: BackgroundPolicy,
    pub battery_background: BackgroundPolicy,
}

impl Default for PowerPolicyConfig {
    fn default() -> Self {
        Self {
            thresholds: PowerThresholds::default(),
            low_action: LowBatteryAction::Suspend,
            swap_available: false,
            suspend_then_hibernate: false,
            ac_background: BackgroundPolicy::ac(),
            battery_background: BackgroundPolicy::battery(),
        }
    }
}

/// Power policy implementation.
#[derive(Debug)]
pub struct PowerPolicyImpl {
    config: PowerPolicyConfig,
    source: PowerSource,
    profile: PowerProfile,
    user_ac: Option<PowerProfile>,
    user_battery: Option<PowerProfile>,
    low: LowBatteryMachine,
    charging: bool,
    lid_closed: bool,
    thermal: ThermalLevel,
    charge_limit: Option<u8>,
    last_percent: f32,
}

impl PowerPolicyImpl {
    pub fn new(config: PowerPolicyConfig) -> Self {
        Self {
            config,
            source: PowerSource::Unknown,
            profile: PowerProfile::Balanced,
            user_ac: None,
            user_battery: None,
            low: LowBatteryMachine::new(
                config.thresholds,
                config.low_action.clone(),
                config.swap_available,
            ),
            charging: false,
            lid_closed: false,
            thermal: ThermalLevel::Normal,
            charge_limit: None,
            last_percent: 100.0,
        }
    }

    pub fn profile(&self) -> PowerProfile {
        self.profile
    }

    pub fn source(&self) -> PowerSource {
        self.source
    }

    pub fn low_state(&self) -> LowBatteryState {
        self.low.state()
    }

    pub fn set_charge_limit(&mut self, limit: Option<u8>) {
        self.charge_limit = limit.map(clamp_charge_limit);
    }

    pub fn set_low_action(&mut self, action: LowBatteryAction) {
        self.low.set_low_action(action);
    }

    fn background_for(&self, source: PowerSource) -> BackgroundPolicy {
        match source {
            PowerSource::Battery => self.config.battery_background,
            _ => self.config.ac_background,
        }
    }

    fn set_profile(&mut self, profile: PowerProfile, reason: &str) -> Vec<PowerAction> {
        if self.profile == profile {
            return Vec::new();
        }
        let previous = self.profile;
        self.profile = profile;
        vec![
            PowerAction::SetProfile(profile),
            PowerAction::Log(format!(
                "profile {previous:?}->{profile:?} reason={reason}"
            )),
        ]
    }

    fn source_changed(&mut self, source: PowerSource) -> Vec<PowerAction> {
        let previous = self.source;
        self.source = source;
        let mut actions = Vec::new();
        if source == PowerSource::Ac {
            self.low.reset();
        }
        let target = match source {
            PowerSource::Ac => self.user_ac.unwrap_or(PowerProfile::Balanced),
            PowerSource::Battery => self.user_battery.unwrap_or(PowerProfile::Saver),
            PowerSource::Unknown => self.profile,
        };
        actions.extend(self.set_profile(target, &format!("source_changed({source:?})")));
        actions.push(PowerAction::SetBackgroundPolicy(self.background_for(source)));
        actions.push(PowerAction::Log(format!(
            "source {previous:?}->{source:?}"
        )));
        actions
    }

    fn battery_updated(&mut self, status: BatteryStatus) -> Vec<PowerAction> {
        self.charging = status.charging;
        self.last_percent = status.percent;
        if status.source != PowerSource::Unknown && status.source != self.source {
            // UPower may learn the source before logind does.
            let mut actions = self.source_changed(status.source);
            actions.extend(self.low_battery_actions(status));
            return actions;
        }
        self.low_battery_actions(status)
    }

    fn low_battery_actions(&mut self, status: BatteryStatus) -> Vec<PowerAction> {
        let discharging = status.source == PowerSource::Battery && !status.charging;
        if !discharging {
            return Vec::new();
        }
        let mut actions = Vec::new();
        for decision in self.low.update(status.percent, status.charging) {
            actions.push(PowerAction::Notify(decision.notify));
            if let Some(profile) = decision.profile {
                actions.extend(self.set_profile(profile, "low_battery"));
            }
            if let Some(sleep) = decision.sleep {
                actions.push(PowerAction::SetLowBatteryAction(sleep.clone()));
                actions.push(match sleep {
                    LowBatteryAction::Suspend => PowerAction::Suspend,
                    LowBatteryAction::Hibernate => PowerAction::Hibernate,
                });
            }
        }
        actions
    }
}

pub fn clamp_charge_limit(limit: u8) -> u8 {
    limit.clamp(40, 100)
}

impl PowerPolicy for PowerPolicyImpl {
    fn on_event(&mut self, event: PowerEvent) -> Vec<PowerAction> {
        match event {
            PowerEvent::SourceChanged(source) => self.source_changed(source),
            PowerEvent::BatteryUpdated(status) => self.battery_updated(status),
            PowerEvent::LidChanged { closed } => {
                self.lid_closed = closed;
                if !closed {
                    return vec![PowerAction::Log("lid opened".into())];
                }
                if self.charging || self.source == PowerSource::Ac {
                    return vec![PowerAction::Log(
                        "lid closed while charging: no sleep".into(),
                    )];
                }
                let mut actions = Vec::new();
                if self.config.suspend_then_hibernate {
                    actions.push(PowerAction::Log(
                        "lid closed: suspend-then-hibernate requested".into(),
                    ));
                } else {
                    actions.push(PowerAction::Log("lid closed: suspend".into()));
                }
                actions.push(PowerAction::Suspend);
                actions
            }
            PowerEvent::UserProfileRequest(profile) => {
                match self.source {
                    PowerSource::Ac => self.user_ac = Some(profile),
                    PowerSource::Battery => self.user_battery = Some(profile),
                    PowerSource::Unknown => {}
                }
                self.set_profile(profile, "user_request")
            }
            PowerEvent::ThermalChanged(level) => {
                self.thermal = level;
                if level == ThermalLevel::Hot && self.profile == PowerProfile::Performance {
                    let mut actions = self.set_profile(PowerProfile::Balanced, "thermal_hot");
                    actions.push(PowerAction::Notify(
                        "The system is hot; switched to Balanced.".into(),
                    ));
                    return actions;
                }
                vec![PowerAction::Log(format!("thermal level {level:?}"))]
            }
            PowerEvent::UserIdle(idle) => vec![PowerAction::Log(format!(
                "user idle={idle} (profile stays {:?})",
                self.profile
            ))],
            PowerEvent::DeviceError { device } => vec![PowerAction::Log(format!(
                "device error reported for {device}"
            ))],
            PowerEvent::ChargeLimitRequest(percent) => {
                let limit = clamp_charge_limit(percent);
                self.charge_limit = Some(limit);
                vec![
                    PowerAction::SetChargeLimit(limit),
                    PowerAction::Log(format!("charge limit requested: {limit}%")),
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percent: f32, charging: bool) -> BatteryStatus {
        BatteryStatus {
            percent,
            source: if charging {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
            charging,
            ..Default::default()
        }
    }

    fn profile_actions(actions: &[PowerAction]) -> Vec<PowerProfile> {
        actions
            .iter()
            .filter_map(|a| match a {
                PowerAction::SetProfile(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn source_changes_switch_profiles_and_background_policy() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        let actions = policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));
        assert_eq!(profile_actions(&actions), vec![PowerProfile::Saver]);
        assert!(actions.contains(&PowerAction::SetBackgroundPolicy(
            BackgroundPolicy::battery()
        )));

        let actions = policy.on_event(PowerEvent::SourceChanged(PowerSource::Ac));
        assert_eq!(profile_actions(&actions), vec![PowerProfile::Balanced]);
        assert!(actions.contains(&PowerAction::SetBackgroundPolicy(
            BackgroundPolicy::ac()
        )));
    }

    #[test]
    fn user_choice_survives_source_changes() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Ac));
        let actions = policy.on_event(PowerEvent::UserProfileRequest(
            PowerProfile::Performance,
        ));
        assert_eq!(profile_actions(&actions), vec![PowerProfile::Performance]);

        policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));
        assert_eq!(policy.profile(), PowerProfile::Saver);
        let actions = policy.on_event(PowerEvent::SourceChanged(PowerSource::Ac));
        assert_eq!(profile_actions(&actions), vec![PowerProfile::Performance]);
    }

    #[test]
    fn thermal_hot_caps_performance_at_balanced() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Ac));
        policy.on_event(PowerEvent::UserProfileRequest(PowerProfile::Performance));
        let actions = policy.on_event(PowerEvent::ThermalChanged(ThermalLevel::Hot));
        assert_eq!(policy.profile(), PowerProfile::Balanced);
        assert!(actions
            .iter()
            .any(|a| matches!(a, PowerAction::Notify(text) if text.contains("hot"))));
    }

    #[test]
    fn low_battery_runs_each_threshold_once_per_cycle() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));

        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(21.0, false)));
        assert!(actions.is_empty());

        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(19.0, false)));
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a, PowerAction::Notify(_)))
                .count(),
            1
        );
        assert_eq!(policy.low_state(), LowBatteryState::Warned20);

        // The same threshold does not fire twice.
        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(18.0, false)));
        assert!(actions.is_empty());

        // Profile is already Saver on battery, so no duplicate SetProfile.
        policy.on_event(PowerEvent::BatteryUpdated(battery(9.0, false)));
        assert_eq!(policy.profile(), PowerProfile::Saver);
        assert_eq!(policy.low_state(), LowBatteryState::Saver10);

        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(4.0, false)));
        assert!(actions.contains(&PowerAction::Suspend));
        assert_eq!(policy.low_state(), LowBatteryState::ActionDue5);

        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(2.0, false)));
        assert!(actions.contains(&PowerAction::Suspend));
        assert_eq!(policy.low_state(), LowBatteryState::Critical3);
    }

    #[test]
    fn low_battery_machine_resets_when_charging() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));
        policy.on_event(PowerEvent::BatteryUpdated(battery(4.0, false)));
        assert_ne!(policy.low_state(), LowBatteryState::Normal);

        policy.on_event(PowerEvent::BatteryUpdated(battery(4.0, true)));
        assert_eq!(policy.low_state(), LowBatteryState::Normal);

        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(4.0, false)));
        assert!(actions.contains(&PowerAction::Suspend));
    }

    #[test]
    fn critical_threshold_hibernates_when_swap_exists() {
        let config = PowerPolicyConfig {
            swap_available: true,
            low_action: LowBatteryAction::Suspend,
            ..Default::default()
        };
        let mut policy = PowerPolicyImpl::new(config);
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));
        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(2.0, false)));
        assert!(actions.contains(&PowerAction::Hibernate));
    }

    #[test]
    fn configured_low_action_is_used_at_five_percent() {
        let config = PowerPolicyConfig {
            low_action: LowBatteryAction::Hibernate,
            swap_available: true,
            ..Default::default()
        };
        let mut policy = PowerPolicyImpl::new(config);
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));
        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(4.0, false)));
        assert!(actions.contains(&PowerAction::Hibernate));
    }

    #[test]
    fn lid_close_suspends_only_when_not_charging() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Battery));
        policy.on_event(PowerEvent::BatteryUpdated(battery(80.0, false)));
        let actions = policy.on_event(PowerEvent::LidChanged { closed: true });
        assert!(actions.contains(&PowerAction::Suspend));

        policy.on_event(PowerEvent::BatteryUpdated(battery(80.0, true)));
        let actions = policy.on_event(PowerEvent::LidChanged { closed: true });
        assert!(!actions.contains(&PowerAction::Suspend));
    }

    #[test]
    fn charge_limit_is_clamped_and_applied() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        let actions = policy.on_event(PowerEvent::ChargeLimitRequest(20));
        assert!(actions.contains(&PowerAction::SetChargeLimit(40)));
        let actions = policy.on_event(PowerEvent::ChargeLimitRequest(120));
        assert!(actions.contains(&PowerAction::SetChargeLimit(100)));
    }

    #[test]
    fn battery_event_with_new_source_switches_profile() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        let actions = policy.on_event(PowerEvent::BatteryUpdated(battery(50.0, false)));
        assert!(profile_actions(&actions).contains(&PowerProfile::Saver));
        assert!(actions.iter().any(|a| matches!(
            a,
            PowerAction::SetBackgroundPolicy(p) if *p == BackgroundPolicy::battery()
        )));
    }

    #[test]
    fn unknown_source_keeps_current_profile() {
        let mut policy = PowerPolicyImpl::new(PowerPolicyConfig::default());
        policy.on_event(PowerEvent::SourceChanged(PowerSource::Ac));
        let before = policy.profile();
        let actions = policy.on_event(PowerEvent::SourceChanged(PowerSource::Unknown));
        assert_eq!(policy.profile(), before);
        assert!(profile_actions(&actions).is_empty());
    }
}
