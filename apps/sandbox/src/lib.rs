//! AetherOS sandbox, permission broker and profiles (P6-T04).
//!
//! Enforcement is mandatory: when bwrap or user namespaces are unavailable,
//! launch fails with an explanation instead of starting the app unsandboxed.

pub mod enforce;
pub mod permissions;
pub mod profile;

pub use enforce::{Enforcer, SandboxCapabilities, SandboxError};
pub use permissions::{
    describe, AllowListPrompt, AutoPrompt, Broker, PermissionPrompt, PermissionStore,
};
pub use profile::{Sandbox, SandboxProfile};
