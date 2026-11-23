//! Rendering engine adapter for the AetherOS browser (P4-T18 / T18a).
//!
//! ## Engine decision (P4-T18)
//!
//! The default engine is **WebKitGTK 6.0** embedded through `webkit2gtk`, for
//! three reasons from the planning docs: it is the lightest engine that still
//! renders the modern web acceptably on the reference laptop, it integrates
//! with Wayland through GTK's native surfaces, and it ships under LGPL/BSD
//! terms we can redistribute. The full embedding arrives with the hardware
//! session (P2/P3 bring-up, which owns the GPU and compositor surfaces); this
//! module is the seam so the chrome, tabs and data model are finished and
//! testable today.
//!
//! Codec policy for T18a (P0-T03 legal note): the public image ships only
//! royalty-free codecs (VP9/Opus/AV1). H.264/AAC streams fall back to the
//! reduced-quality path or an explicit "codec unavailable" page; DRM (EME)
//! stays off unless the owner enables it.
//!
//! Two adapters exist now:
//! * [`ExternalEngine`] launches a helper renderer process (`AETHER_BROWSER_ENGINE`
//!   or `xdg-open`), which is enough to hand a URL to a system browser while the
//!   embedded surface is being built.
//! * [`NullEngine`] records loads for tests and headless screenshots.

use std::process::Command;

/// Engine surface used by the browser chrome.
pub trait Engine {
    /// Start loading a URL.
    fn load(&mut self, url: &str) -> Result<(), EngineError>;
    /// Stop the current load.
    fn stop(&mut self) -> Result<(), EngineError>;
    /// Reload the current page.
    fn reload(&mut self) -> Result<(), EngineError>;
    /// Human-readable adapter name for the status bar.
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    NotAvailable(String),
    Spawn(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::NotAvailable(what) => write!(f, "engine not available: {what}"),
            EngineError::Spawn(message) => write!(f, "engine spawn failed: {message}"),
        }
    }
}

/// Record-only engine for tests and screenshots.
#[derive(Debug, Default, Clone)]
pub struct NullEngine {
    pub loaded: Vec<String>,
    pub reloads: usize,
    pub stops: usize,
}

impl NullEngine {
    pub fn new() -> NullEngine {
        NullEngine::default()
    }
}

impl Engine for NullEngine {
    fn load(&mut self, url: &str) -> Result<(), EngineError> {
        self.loaded.push(url.to_string());
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.stops += 1;
        Ok(())
    }

    fn reload(&mut self) -> Result<(), EngineError> {
        self.reloads += 1;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "null"
    }
}

/// Hands URLs to an external renderer process.
#[derive(Debug, Clone)]
pub struct ExternalEngine {
    pub program: String,
    pub launched: Vec<String>,
}

impl ExternalEngine {
    /// `AETHER_BROWSER_ENGINE` if set, otherwise `xdg-open` on Linux and `open`
    /// on macOS so the dev host works too.
    pub fn from_environment() -> ExternalEngine {
        let program = std::env::var("AETHER_BROWSER_ENGINE").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "open".to_string()
            } else {
                "xdg-open".to_string()
            }
        });
        ExternalEngine {
            program,
            launched: Vec::new(),
        }
    }

    pub fn with_program(program: &str) -> ExternalEngine {
        ExternalEngine {
            program: program.to_string(),
            launched: Vec::new(),
        }
    }
}

/// Build the argv used to hand a URL to the engine.
pub fn engine_argv(program: &str, url: &str) -> Vec<String> {
    vec![program.to_string(), url.to_string()]
}

impl Engine for ExternalEngine {
    fn load(&mut self, url: &str) -> Result<(), EngineError> {
        let argv = engine_argv(&self.program, url);
        let (program, args) = argv.split_first().expect("argv always has the program");
        Command::new(program)
            .args(args)
            .spawn()
            .map_err(|error| EngineError::Spawn(format!("{program}: {error}")))?;
        self.launched.push(url.to_string());
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        Ok(())
    }

    fn reload(&mut self) -> Result<(), EngineError> {
        Err(EngineError::NotAvailable(
            "external engine cannot reload".to_string(),
        ))
    }

    fn name(&self) -> &'static str {
        "external"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_engine_records_calls() {
        let mut engine = NullEngine::new();
        engine.load("https://a.example/").unwrap();
        engine.reload().unwrap();
        engine.stop().unwrap();
        assert_eq!(engine.loaded, ["https://a.example/"]);
        assert_eq!(engine.reloads, 1);
        assert_eq!(engine.stops, 1);
    }

    #[test]
    fn engine_argv_is_program_plus_url() {
        assert_eq!(
            engine_argv("webkitgtk-launcher", "https://a.example/"),
            ["webkitgtk-launcher", "https://a.example/"]
        );
    }

    #[test]
    fn external_engine_reports_a_missing_program() {
        let mut engine = ExternalEngine::with_program("aether-no-such-engine-binary");
        let error = engine.load("https://a.example/").unwrap_err();
        assert!(matches!(error, EngineError::Spawn(_)));
        assert!(matches!(
            engine.reload(),
            Err(EngineError::NotAvailable(_))
        ));
    }

    #[test]
    fn environment_defaults_to_a_known_opener() {
        let engine = ExternalEngine::from_environment();
        assert!(!engine.program.is_empty());
    }
}
