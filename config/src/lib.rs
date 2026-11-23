//! AetherOS layered configuration, theme tokens, keybindings, Lua hooks and
//! optional feature packs (P7-T01..T07, F4/F15).
//!
//! Files first: every setting is a TOML key registered in [`schema::SCHEMA`],
//! layered defaults → `/etc/aether` → `~/.config/aether`. Validation rejects
//! invalid values and keeps the last valid configuration active.

pub mod error;
pub mod features;
pub mod hooks;
pub mod keybindings;
pub mod schema;
pub mod store;
pub mod theme;

pub use error::{ConfigError, HookError};
pub use features::{
    catalog as feature_catalog, pack as feature_pack, FeatureError, FeatureManager, FeaturePack,
    FeatureRow, FeatureState, IdleBudget,
};
pub use hooks::{
    discover as discover_hooks, HookEvent, HookManager, HookResult, HookRunner, HooksError,
    LuaCliRunner,
};
pub use keybindings::{normalize_key, Binding, BindingError, BindingSet};
pub use schema::{schema_files, spec_for, KeySpec, ValueKind, SCHEMA};
pub use store::{parse_typed, ConfigStore, Layer, MergedValue, Roots};
pub use theme::{tokens_from_toml, ThemeEngine, ThemeReload, ThemeTokens};
