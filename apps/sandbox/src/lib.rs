//! AetherOS sandbox, permission broker and profiles (P6-T04).
//!
//! Enforcement is mandatory: when bwrap, user namespaces or Landlock are
//! unavailable, launch fails with an explanation instead of starting the app
//! unsandboxed (fail-closed, D8 section 5).

pub mod enforce;
pub mod landlock;
pub mod permissions;
pub mod profile;

pub use enforce::{Enforcer, SandboxCapabilities, SandboxError};
pub use landlock::{
    LandlockAccess, LandlockApplied, LandlockPlan, LandlockPolicy, LandlockRule, LandlockStatus,
};
pub use permissions::{
    describe, AllowListPrompt, AutoPrompt, Broker, PermissionPrompt, PermissionStore,
};
pub use profile::{Sandbox, SandboxProfile};
