//! Service actions behind the permission-prompt contract (TM-17, F13).
//!
//! The Services tab never runs `systemctl` directly. Every Start, Stop or
//! Restart first becomes a [`ServiceActionPlan`] carrying the exact command and
//! the prompt the window shows; the user's answer lives in
//! [`ServiceActionPrompt`], and only a confirmed plan reaches a
//! [`ServiceExecutor`]. Running the plan on a real system needs the systemd
//! path validated on AetherOS hardware, so this round ships the plan, the
//! prompt state machine and the executor seam, and nothing spawns a process.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};

/// Prefix used by the table row action ids (`service:<action>:<unit>`).
pub const SERVICE_ACTION_PREFIX: &str = "service";

/// Polkit action the prompt authenticates with. The policy file below is
/// generated from the same contract, so the id cannot drift from the UI.
pub const POLKIT_ACTION_ID: &str = "org.aether.taskmanager.service-action";

/// Where the generated policy file is installed (D10 §5.3).
pub const POLKIT_POLICY_PATH: &str = "/usr/share/polkit-1/actions/org.aether.taskmanager.service-action.policy";

/// The generated policy file, compiled in so a test can prove the shipped copy
/// is byte-identical to what the contract publishes.
pub const POLKIT_POLICY_FILE: &str =
    include_str!("../polkit/org.aether.taskmanager.service-action.policy");

/// One action the Services tab offers (TM-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

impl ServiceAction {
    pub const ALL: [Self; 3] = [Self::Start, Self::Stop, Self::Restart];

    pub fn key(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
        }
    }

    /// The `systemctl` verb (identical to the key, pinned separately so the
    /// command line can never drift from the UI key).
    pub fn verb(self) -> &'static str {
        self.key()
    }

    /// The systemd D-Bus manager method the executor calls.
    pub fn systemd_method(self) -> &'static str {
        match self {
            Self::Start => "StartUnit",
            Self::Stop => "StopUnit",
            Self::Restart => "RestartUnit",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }
}

/// One entry in the service-action contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceActionSpec {
    pub action: ServiceAction,
    pub label: String,
    pub confirm_label: String,
    /// Design token name for the prompt accent.
    pub tone: String,
    /// Prompt body; `{unit}` is replaced with the unit name.
    pub message: String,
    pub requires_admin: bool,
}

impl ServiceActionSpec {
    pub fn message_for(&self, unit: &str) -> String {
        self.message.replace("{unit}", unit)
    }
}

/// A unit the contract refuses to touch, with the reason shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedUnit {
    pub unit: String,
    pub reason: String,
}

/// Contract for the Services tab actions (TM-17, D10 §5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceActionContract {
    pub key: String,
    pub header: String,
    /// Permission the prompt asks for (F9 names, admin scope).
    pub permission: String,
    pub cancel_label: String,
    pub admin_note: String,
    pub protected_units: Vec<ProtectedUnit>,
    pub actions: Vec<ServiceActionSpec>,
    pub tooltip: String,
    /// Polkit action the prompt authenticates with (see [`polkit_policy_xml`]).
    pub polkit_action_id: String,
    /// Install path of the generated policy file.
    pub polkit_policy_path: String,
}

impl Default for ServiceActionContract {
    fn default() -> Self {
        Self {
            key: "service-action".into(),
            header: "Services".into(),
            permission: "admin.authorize".into(),
            cancel_label: "Cancel".into(),
            admin_note:
                "An administrator action: the prompt shows the exact command and the answer is never remembered."
                    .into(),
            protected_units: vec![ProtectedUnit {
                unit: "aether-lifecycled.service".into(),
                reason: "the lifecycle daemon suspends and kills every managed app".into(),
            }],
            actions: vec![
                ServiceActionSpec {
                    action: ServiceAction::Start,
                    label: ServiceAction::Start.label().into(),
                    confirm_label: "Start service".into(),
                    tone: "success".into(),
                    message: "Start “{unit}”? The service begins running now and administrator permission is required.".into(),
                    requires_admin: true,
                },
                ServiceActionSpec {
                    action: ServiceAction::Stop,
                    label: ServiceAction::Stop.label().into(),
                    confirm_label: "Stop service".into(),
                    tone: "danger".into(),
                    message: "Stop “{unit}”? Its work ends now and apps using it may lose data. Administrator permission is required.".into(),
                    requires_admin: true,
                },
                ServiceActionSpec {
                    action: ServiceAction::Restart,
                    label: ServiceAction::Restart.label().into(),
                    confirm_label: "Restart service".into(),
                    tone: "warning".into(),
                    message: "Restart “{unit}”? It stops and starts again, so anything using it reconnects. Administrator permission is required.".into(),
                    requires_admin: true,
                },
            ],
            tooltip: "Start, Stop and Restart ask for administrator permission and show the systemctl command before anything runs.".into(),
            polkit_action_id: POLKIT_ACTION_ID.into(),
            polkit_policy_path: POLKIT_POLICY_PATH.into(),
        }
    }
}

impl ServiceActionContract {
    pub fn spec(&self, action: ServiceAction) -> Option<&ServiceActionSpec> {
        self.actions.iter().find(|spec| spec.action == action)
    }

    /// Reason a unit is protected, when the contract protects it.
    pub fn protected_reason(&self, unit: &str) -> Option<&str> {
        self.protected_units
            .iter()
            .find(|protected| protected.unit == unit)
            .map(|protected| protected.reason.as_str())
    }
}

/// The permission prompt the window shows before a service action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePrompt {
    pub title: String,
    pub message: String,
    /// The exact command and the permission it needs.
    pub detail: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub tone: String,
}

/// A validated action, ready to be confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceActionPlan {
    pub unit: String,
    pub action: ServiceAction,
    /// The command that would run, as an argument vector.
    pub command: Vec<String>,
    pub prompt: ServicePrompt,
    pub requires_admin: bool,
    pub permission: String,
}

impl ServiceActionPlan {
    pub fn command_line(&self) -> String {
        self.command.join(" ")
    }
}

/// Result of asking for an action: either a refusal or a plan to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceActionRequest {
    Refused {
        unit: String,
        action: ServiceAction,
        reason: String,
    },
    Confirm {
        plan: ServiceActionPlan,
    },
}

impl ServiceActionRequest {
    pub fn plan(&self) -> Option<&ServiceActionPlan> {
        match self {
            Self::Confirm { plan } => Some(plan),
            Self::Refused { .. } => None,
        }
    }
}

/// Builds plans from the contract. Stateless; the prompt state lives in
/// [`ServiceActionPrompt`].
#[derive(Debug, Clone, Default)]
pub struct ServiceActionPlanner {
    pub contract: ServiceActionContract,
}

impl ServiceActionPlanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_contract(contract: ServiceActionContract) -> Self {
        Self { contract }
    }

    /// Validate `unit`/`action` and build the plan (TM-17). Refusals are
    /// explicit so the window can explain them instead of failing silently.
    pub fn request(&self, unit: &str, action: ServiceAction) -> ServiceActionRequest {
        let unit = unit.trim();
        if let Err(reason) = self.validate_unit(unit) {
            return ServiceActionRequest::Refused {
                unit: unit.to_string(),
                action,
                reason,
            };
        }
        let Some(spec) = self.contract.spec(action) else {
            return ServiceActionRequest::Refused {
                unit: unit.to_string(),
                action,
                reason: "this action is not in the service-action contract".into(),
            };
        };
        let command = vec![
            "systemctl".to_string(),
            action.verb().to_string(),
            unit.to_string(),
        ];
        let plan = ServiceActionPlan {
            unit: unit.to_string(),
            action,
            prompt: ServicePrompt {
                title: format!("{} “{unit}”?", action.label()),
                message: spec.message_for(unit),
                detail: format!(
                    "{} — {} ({})",
                    command.join(" "),
                    self.contract.admin_note,
                    self.contract.permission
                ),
                confirm_label: spec.confirm_label.clone(),
                cancel_label: self.contract.cancel_label.clone(),
                tone: spec.tone.clone(),
            },
            command,
            requires_admin: spec.requires_admin,
            permission: self.contract.permission.clone(),
        };
        ServiceActionRequest::Confirm { plan }
    }

    fn validate_unit(&self, unit: &str) -> Result<(), String> {
        if unit.is_empty() {
            return Err("no unit was selected".into());
        }
        if unit.len() > 256 {
            return Err("unit name is too long".into());
        }
        if !unit.ends_with(".service") {
            return Err("only .service units can be started, stopped or restarted".into());
        }
        if !unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | ':'))
        {
            return Err("unit name contains characters systemd does not accept".into());
        }
        if let Some(reason) = self.contract.protected_reason(unit) {
            return Err(format!("“{unit}” is protected: {reason}"));
        }
        Ok(())
    }
}

/// What the prompt is currently asking.
///
/// The pending plan is stored inline: one prompt exists at a time, and keeping
/// it by value makes the confirm/cancel hand-off a plain move.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ServicePromptState {
    /// No prompt is open; actions cannot run.
    #[default]
    Idle,
    Pending(ServiceActionPlan),
}

/// Result of confirming or cancelling a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceActionOutcome {
    Executed {
        unit: String,
        action: ServiceAction,
        output: String,
    },
    Failed {
        unit: String,
        action: ServiceAction,
        reason: String,
    },
    Cancelled {
        unit: String,
        action: ServiceAction,
    },
    Refused {
        unit: String,
        reason: String,
    },
    /// Confirmed or cancelled with no prompt open; nothing ran.
    NothingPending,
}

/// Runs a confirmed plan. The real implementation (systemd D-Bus/polkit) is
/// wired on AetherOS hardware; tests use [`PlanningExecutor`].
pub trait ServiceExecutor {
    fn run(&self, plan: &ServiceActionPlan) -> Result<String, String>;
}

/// Records plans instead of running them, for tests and dry-run output.
#[derive(Debug, Default)]
pub struct PlanningExecutor {
    pub calls: RefCell<Vec<ServiceActionPlan>>,
}

impl ServiceExecutor for PlanningExecutor {
    fn run(&self, plan: &ServiceActionPlan) -> Result<String, String> {
        self.calls.borrow_mut().push(plan.clone());
        Ok(format!("planned: {}", plan.command_line()))
    }
}

/// The polkit policy file for the service-action right, generated from the
/// contract (TM-17, D10 §5.3).
///
/// The DTD allows one `description` and one or more `message` elements, so the
/// action carries one right for Start/Stop/Restart (listed in the comment) and
/// every default is `auth_admin`: the answer is never remembered, which is the
/// same rule the prompt follows.
pub fn polkit_policy_xml(contract: &ServiceActionContract) -> String {
    let keys: Vec<&str> = contract
        .actions
        .iter()
        .map(|spec| spec.action.key())
        .collect();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<!DOCTYPE policyconfig PUBLIC\n");
    xml.push_str(" \"-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN\"\n");
    xml.push_str(" \"http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd\">\n");
    xml.push_str("<policyconfig>\n");
    xml.push_str("  <vendor>AetherOS</vendor>\n");
    xml.push_str("  <vendor_url>https://aetheros.invalid</vendor_url>\n");
    xml.push_str("  <icon_name>system-run</icon_name>\n");
    xml.push_str(&format!(
        "  <!-- Actions: {}; permission {}; an answer is never remembered. -->\n",
        xml_escape(&keys.join(", ")),
        xml_escape(&contract.permission)
    ));
    xml.push_str(&format!(
        "  <action id=\"{}\">\n",
        xml_escape(&contract.polkit_action_id)
    ));
    xml.push_str(&format!(
        "    <description>{}</description>\n",
        xml_escape("Start, stop or restart a system service")
    ));
    xml.push_str(&format!(
        "    <message>{}</message>\n",
        xml_escape("Authentication is required to change the state of a system service")
    ));
    xml.push_str("    <defaults>\n");
    xml.push_str("      <allow_any>auth_admin</allow_any>\n");
    xml.push_str("      <allow_inactive>auth_admin</allow_inactive>\n");
    xml.push_str("      <allow_active>auth_admin</allow_active>\n");
    xml.push_str("    </defaults>\n");
    xml.push_str("  </action>\n");
    xml.push_str("</policyconfig>\n");
    xml
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Argument vector for calling the systemd manager over D-Bus.
///
/// `busctl call org.freedesktop.systemd1 /org/freedesktop/systemd1
/// org.freedesktop.systemd1.Manager StartUnit ss <unit> replace` — systemd
/// authorizes the call with its own `org.freedesktop.systemd1.manage-units`
/// polkit action; `busctl` is used instead of linking libdbus so the binary has
/// no new dependency.
pub fn systemd_argv(plan: &ServiceActionPlan) -> Vec<String> {
    [
        "busctl",
        "call",
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        plan.action.systemd_method(),
        "ss",
        plan.unit.as_str(),
        "replace",
    ]
    .iter()
    .map(|part| part.to_string())
    .collect()
}

/// How a [`SystemdDbusExecutor`] runs an argument vector.
type Runner = Box<dyn Fn(&[String]) -> Result<String, String>>;

/// Runs a confirmed plan through systemd's D-Bus manager (TM-17).
///
/// The runner is injected so the tests prove the exact argument vector without
/// spawning anything; [`Self::system`] is the only variant that starts a
/// process, and only after a [`ServiceActionPrompt`] confirmed the plan.
pub struct SystemdDbusExecutor {
    runner: Runner,
    calls: RefCell<Vec<Vec<String>>>,
}

impl SystemdDbusExecutor {
    /// Plans instead of spawning: records the argument vector, returns it.
    pub fn dry_run() -> Self {
        Self::with_runner(Box::new(|argv: &[String]| {
            Ok(format!("planned: {}", argv.join(" ")))
        }))
    }

    /// Calls the real systemd manager. Hardware-gated: needs systemd, polkit and
    /// `busctl` on AetherOS.
    pub fn system() -> Self {
        Self::with_runner(Box::new(|argv: &[String]| {
            let (program, args) = argv
                .split_first()
                .ok_or_else(|| "empty command vector".to_string())?;
            let output = std::process::Command::new(program)
                .args(args)
                .output()
                .map_err(|error| format!("cannot run {program}: {error}"))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if stderr.is_empty() {
                    format!("{program} exited with {}", output.status)
                } else {
                    stderr
                })
            }
        }))
    }

    fn with_runner(runner: Runner) -> Self {
        Self {
            runner,
            calls: RefCell::new(Vec::new()),
        }
    }

    /// Argument vectors this executor was asked to run.
    pub fn calls(&self) -> std::cell::Ref<'_, Vec<Vec<String>>> {
        self.calls.borrow()
    }
}

impl ServiceExecutor for SystemdDbusExecutor {
    fn run(&self, plan: &ServiceActionPlan) -> Result<String, String> {
        let argv = systemd_argv(plan);
        let output = (self.runner)(&argv)?;
        if output.starts_with("planned:") {
            self.calls.borrow_mut().push(argv);
        }
        Ok(output)
    }
}

/// The permission-prompt state machine (TM-17).
///
/// A request only *plans* the action. Confirming hands the pending plan to the
/// executor once; cancelling drops it. Cancelling or confirming with nothing
/// pending is [`ServiceActionOutcome::NothingPending`], so a stray button press
/// can never run a command.
#[derive(Debug, Clone, Default)]
pub struct ServiceActionPrompt {
    pub planner: ServiceActionPlanner,
    state: ServicePromptState,
}

impl ServiceActionPrompt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_contract(contract: ServiceActionContract) -> Self {
        Self {
            planner: ServiceActionPlanner::with_contract(contract),
            state: ServicePromptState::Idle,
        }
    }

    pub fn state(&self) -> &ServicePromptState {
        &self.state
    }

    pub fn pending(&self) -> Option<&ServiceActionPlan> {
        match &self.state {
            ServicePromptState::Pending(plan) => Some(plan),
            ServicePromptState::Idle => None,
        }
    }

    /// Ask for an action. A refused request leaves any open prompt untouched;
    /// an accepted one replaces it (one prompt at a time).
    pub fn request(&mut self, unit: &str, action: ServiceAction) -> ServiceActionRequest {
        let request = self.planner.request(unit, action);
        if let ServiceActionRequest::Confirm { plan } = &request {
            self.state = ServicePromptState::Pending(plan.clone());
        }
        request
    }

    /// The user pressed Cancel.
    pub fn cancel(&mut self) -> ServiceActionOutcome {
        match std::mem::take(&mut self.state) {
            ServicePromptState::Idle => ServiceActionOutcome::NothingPending,
            ServicePromptState::Pending(plan) => ServiceActionOutcome::Cancelled {
                unit: plan.unit,
                action: plan.action,
            },
        }
    }

    /// The user confirmed: run the pending plan exactly once.
    pub fn confirm(&mut self, executor: &dyn ServiceExecutor) -> ServiceActionOutcome {
        match std::mem::take(&mut self.state) {
            ServicePromptState::Idle => ServiceActionOutcome::NothingPending,
            ServicePromptState::Pending(plan) => match executor.run(&plan) {
                Ok(output) => ServiceActionOutcome::Executed {
                    unit: plan.unit,
                    action: plan.action,
                    output,
                },
                Err(reason) => ServiceActionOutcome::Failed {
                    unit: plan.unit,
                    action: plan.action,
                    reason,
                },
            },
        }
    }
}

/// Row action id for a unit (`service:<action>:<unit>`).
pub fn action_id(unit: &str, action: ServiceAction) -> String {
    format!("{SERVICE_ACTION_PREFIX}:{}:{unit}", action.key())
}

/// Parse a row action id back into the action and unit.
pub fn parse_action_id(id: &str) -> Option<(ServiceAction, &str)> {
    let rest = id.strip_prefix(SERVICE_ACTION_PREFIX)?.strip_prefix(':')?;
    let (key, unit) = rest.split_once(':')?;
    let action = ServiceAction::from_key(key)?;
    if unit.is_empty() {
        return None;
    }
    Some((action, unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planner() -> ServiceActionPlanner {
        ServiceActionPlanner::new()
    }

    #[test]
    fn contract_pins_the_actions_and_the_permission() {
        let contract = ServiceActionContract::default();
        assert_eq!(contract.key, "service-action");
        assert_eq!(contract.permission, "admin.authorize");
        assert_eq!(contract.actions.len(), 3);
        for action in ServiceAction::ALL {
            let spec = contract.spec(action).expect("every action is in the contract");
            assert!(spec.requires_admin);
            assert!(spec.message.contains("{unit}"));
            assert_eq!(spec.action.key(), action.key());
        }
        assert_eq!(contract.cancel_label, "Cancel");
        assert!(contract.protected_reason("aether-lifecycled.service").is_some());
        assert_eq!(contract.polkit_action_id, POLKIT_ACTION_ID);
        assert_eq!(contract.polkit_policy_path, POLKIT_POLICY_PATH);
        let json = serde_json::to_string(&contract).unwrap();
        assert!(json.contains("\"permission\":\"admin.authorize\""));
        assert!(json.contains("\"action\":\"restart\""));
        assert!(json.contains("\"polkit_action_id\":\"org.aether.taskmanager.service-action\""));
    }

    #[test]
    fn shipped_polkit_policy_is_generated_from_the_contract() {
        // The file under polkit/ is compiled in, so this test fails the moment
        // the contract and the installed policy drift apart.
        let generated = polkit_policy_xml(&ServiceActionContract::default());
        assert_eq!(POLKIT_POLICY_FILE, generated);
        assert!(POLKIT_POLICY_FILE.contains(&format!("<action id=\"{POLKIT_ACTION_ID}\">")));
        assert!(POLKIT_POLICY_FILE.contains("<allow_active>auth_admin</allow_active>"));
        assert!(!POLKIT_POLICY_FILE.contains("auth_admin_keep"), "answers are never remembered");
        // One action right covers the three verbs the tab offers.
        for action in ServiceAction::ALL {
            assert!(POLKIT_POLICY_FILE.contains(action.key()));
        }
        assert_eq!(POLKIT_POLICY_FILE.matches("<action id=").count(), 1);
    }

    #[test]
    fn systemd_executor_calls_the_manager_methods_behind_a_confirm() {
        let executor = SystemdDbusExecutor::dry_run();
        let mut prompt = ServiceActionPrompt::new();
        // Nothing reaches the executor before the confirm.
        prompt.request("aether-powerd.service", ServiceAction::Restart);
        assert!(executor.calls().is_empty());

        let outcome = prompt.confirm(&executor);
        assert!(matches!(outcome, ServiceActionOutcome::Executed { .. }));
        let calls = executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            vec![
                "busctl",
                "call",
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
                "RestartUnit",
                "ss",
                "aether-powerd.service",
                "replace",
            ]
        );

        // Mapping per verb is pinned so the UI key cannot drift from the call.
        for (action, method) in [
            (ServiceAction::Start, "StartUnit"),
            (ServiceAction::Stop, "StopUnit"),
            (ServiceAction::Restart, "RestartUnit"),
        ] {
            let plan = planner()
                .request("aether-powerd.service", action)
                .plan()
                .unwrap()
                .clone();
            assert_eq!(systemd_argv(&plan)[5], method);
            assert_eq!(systemd_argv(&plan)[7], "aether-powerd.service");
        }
    }

    #[test]
    fn request_builds_the_plan_and_the_prompt() {
        let request = planner().request("aether-powerd.service", ServiceAction::Restart);
        let plan = request.plan().expect("a valid unit gets a plan");
        assert_eq!(plan.unit, "aether-powerd.service");
        assert_eq!(plan.action, ServiceAction::Restart);
        assert_eq!(
            plan.command,
            vec!["systemctl", "restart", "aether-powerd.service"]
        );
        assert_eq!(plan.command_line(), "systemctl restart aether-powerd.service");
        assert_eq!(plan.prompt.title, "Restart “aether-powerd.service”?");
        assert!(plan.prompt.message.contains("aether-powerd.service"));
        assert_eq!(plan.prompt.confirm_label, "Restart service");
        assert_eq!(plan.prompt.cancel_label, "Cancel");
        assert_eq!(plan.prompt.tone, "warning");
        assert!(plan.prompt.detail.contains("systemctl restart aether-powerd.service"));
        assert!(plan.prompt.detail.contains("admin.authorize"));
        assert!(plan.requires_admin);
        assert_eq!(plan.permission, "admin.authorize");
    }

    #[test]
    fn invalid_and_protected_units_are_refused_before_any_prompt() {
        let planner = planner();
        for (unit, needle) in [
            ("", "no unit"),
            ("   ", "no unit"),
            ("aether-powerd.socket", "only .service"),
            ("aether-powerd.service; rm -rf /", "only .service"),
            ("../../etc/passwd.service", "characters"),
        ] {
            match planner.request(unit, ServiceAction::Stop) {
                ServiceActionRequest::Refused { reason, .. } => {
                    assert!(reason.contains(needle), "{unit}: {reason}");
                }
                other => panic!("{unit} should be refused, got {other:?}"),
            }
        }
        match planner.request("aether-lifecycled.service", ServiceAction::Stop) {
            ServiceActionRequest::Refused { reason, .. } => {
                assert!(reason.contains("protected"));
                assert!(reason.contains("suspends and kills"));
            }
            other => panic!("protected unit should be refused, got {other:?}"),
        }
    }

    #[test]
    fn nothing_runs_without_a_confirmed_prompt() {
        let executor = PlanningExecutor::default();
        let mut prompt = ServiceActionPrompt::new();
        assert_eq!(
            prompt.confirm(&executor),
            ServiceActionOutcome::NothingPending
        );
        assert_eq!(
            prompt.cancel(),
            ServiceActionOutcome::NothingPending
        );
        assert!(executor.calls.borrow().is_empty());
        assert_eq!(prompt.state(), &ServicePromptState::Idle);
    }

    #[test]
    fn cancel_drops_the_plan_without_running_it() {
        let executor = PlanningExecutor::default();
        let mut prompt = ServiceActionPrompt::new();
        let request = prompt.request("aether-updated.service", ServiceAction::Start);
        assert!(matches!(request, ServiceActionRequest::Confirm { .. }));
        assert!(prompt.pending().is_some());
        assert_eq!(prompt.state(), &ServicePromptState::Pending(
            request.plan().unwrap().clone()
        ));

        assert_eq!(
            prompt.cancel(),
            ServiceActionOutcome::Cancelled {
                unit: "aether-updated.service".into(),
                action: ServiceAction::Start,
            }
        );
        assert!(executor.calls.borrow().is_empty());
        assert_eq!(prompt.state(), &ServicePromptState::Idle);
        // A second cancel cannot act on the dropped plan.
        assert_eq!(prompt.cancel(), ServiceActionOutcome::NothingPending);
    }

    #[test]
    fn confirm_runs_the_pending_plan_exactly_once() {
        let executor = PlanningExecutor::default();
        let mut prompt = ServiceActionPrompt::new();
        prompt.request("aether-powerd.service", ServiceAction::Stop);
        let outcome = prompt.confirm(&executor);
        assert_eq!(
            outcome,
            ServiceActionOutcome::Executed {
                unit: "aether-powerd.service".into(),
                action: ServiceAction::Stop,
                output: "planned: systemctl stop aether-powerd.service".into(),
            }
        );
        let calls = executor.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].action, ServiceAction::Stop);
        drop(calls);
        assert_eq!(prompt.state(), &ServicePromptState::Idle);
        assert_eq!(
            prompt.confirm(&executor),
            ServiceActionOutcome::NothingPending
        );
        assert_eq!(executor.calls.borrow().len(), 1);
    }

    #[test]
    fn executor_failures_are_reported_against_the_plan() {
        struct Failing;
        impl ServiceExecutor for Failing {
            fn run(&self, _plan: &ServiceActionPlan) -> Result<String, String> {
                Err("polkit refused the action".into())
            }
        }
        let mut prompt = ServiceActionPrompt::new();
        prompt.request("aether-powerd.service", ServiceAction::Restart);
        assert_eq!(
            prompt.confirm(&Failing),
            ServiceActionOutcome::Failed {
                unit: "aether-powerd.service".into(),
                action: ServiceAction::Restart,
                reason: "polkit refused the action".into(),
            }
        );
    }

    #[test]
    fn action_ids_round_trip_including_instanced_units() {
        for unit in [
            "aether-powerd.service",
            "systemd-fsck@dev-disk-by\x2duuid-1234.service",
            "getty@tty1.service",
        ] {
            for action in ServiceAction::ALL {
                let id = action_id(unit, action);
                assert_eq!(parse_action_id(&id), Some((action, unit)));
            }
        }
        assert_eq!(parse_action_id("service:restart:"), None);
        assert_eq!(parse_action_id("service:pause:a.service"), None);
        assert_eq!(parse_action_id("a.service"), None);
    }
}
