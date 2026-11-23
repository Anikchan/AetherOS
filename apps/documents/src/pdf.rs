//! PDF adapter for the Documents app (P4-T13, DA-AC-05).
//!
//! AetherOS keeps no PDF engine of its own in the base image: the viewer drives
//! `poppler-utils` when it is installed (`pdftoppm` renders a page to PNG,
//! `pdftotext` extracts text, `pdfinfo` reports the page count). When the
//! binaries are missing the adapter returns [`PdfError::NotAvailable`] with the
//! list of missing programs — the viewer shows "PDF preview needs poppler-utils"
//! instead of panicking or crashing.
//!
//! The adapter only builds and runs commands; availability detection and the
//! command lines are unit-tested against a fabricated `PATH`, so the tests do
//! not need poppler installed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One external command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl PdfCommand {
    /// Shell-ish representation, useful for logs and error messages.
    pub fn command_line(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// One or more poppler programs are missing from `PATH`.
    NotAvailable { missing: Vec<String> },
    /// The program exists but the call failed.
    Failed(String),
}

impl PdfError {
    pub fn is_not_available(&self) -> bool {
        matches!(self, PdfError::NotAvailable { .. })
    }

    /// Short message the UI shows.
    pub fn message(&self) -> String {
        match self {
            PdfError::NotAvailable { missing } => {
                format!("PDF preview needs poppler-utils (missing: {})", missing.join(", "))
            }
            PdfError::Failed(message) => format!("PDF rendering failed: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PdfAdapter {
    pdftoppm: PathBuf,
    pdftotext: PathBuf,
    pdfinfo: Option<PathBuf>,
}

impl PdfAdapter {
    /// Look for poppler in the process `PATH`.
    pub fn detect() -> Result<PdfAdapter, PdfError> {
        let path = std::env::var("PATH").unwrap_or_default();
        PdfAdapter::detect_in(&path)
    }

    /// Testable availability detection: scan the given `PATH` value.
    pub fn detect_in(path_var: &str) -> Result<PdfAdapter, PdfError> {
        let pdftoppm = find_program(path_var, "pdftoppm");
        let pdftotext = find_program(path_var, "pdftotext");
        let pdfinfo = find_program(path_var, "pdfinfo");

        if pdftoppm.is_none() || pdftotext.is_none() {
            let mut missing = Vec::new();
            if pdftoppm.is_none() {
                missing.push("pdftoppm".to_string());
            }
            if pdftotext.is_none() {
                missing.push("pdftotext".to_string());
            }
            return Err(PdfError::NotAvailable { missing });
        }
        Ok(PdfAdapter {
            pdftoppm: pdftoppm.expect("checked above"),
            pdftotext: pdftotext.expect("checked above"),
            pdfinfo,
        })
    }

    pub fn is_available() -> bool {
        PdfAdapter::detect().is_ok()
    }

    /// `pdftoppm -png -r <dpi> -f <page> -l <page> <pdf> <prefix>`
    pub fn render_command(
        &self,
        pdf: &Path,
        page: u32,
        output_prefix: &Path,
        dpi: u32,
    ) -> PdfCommand {
        PdfCommand {
            program: self.pdftoppm.clone(),
            args: vec![
                "-png".into(),
                "-r".into(),
                dpi.to_string(),
                "-f".into(),
                page.max(1).to_string(),
                "-l".into(),
                page.max(1).to_string(),
                pdf.display().to_string(),
                output_prefix.display().to_string(),
            ],
        }
    }

    /// `pdftotext -layout <pdf> -` (text on stdout).
    pub fn text_command(&self, pdf: &Path) -> PdfCommand {
        PdfCommand {
            program: self.pdftotext.clone(),
            args: vec!["-layout".into(), pdf.display().to_string(), "-".into()],
        }
    }

    /// `pdfinfo <pdf>`; only available when `pdfinfo` is installed.
    pub fn page_count_command(&self, pdf: &Path) -> Option<PdfCommand> {
        self.pdfinfo.as_ref().map(|program| PdfCommand {
            program: program.clone(),
            args: vec![pdf.display().to_string()],
        })
    }

    /// Render one page next to `output_prefix`, returning the produced PNG.
    /// `pdftoppm` appends `-<page>.png` (zero-padded) to the prefix.
    pub fn render_page(
        &self,
        pdf: &Path,
        page: u32,
        output_prefix: &Path,
        dpi: u32,
    ) -> Result<PathBuf, PdfError> {
        run(&self.render_command(pdf, page, output_prefix, dpi))?;
        for candidate in [
            output_prefix.with_extension("png"),
            PathBuf::from(format!("{}-{page}.png", output_prefix.display())),
            PathBuf::from(format!("{}-{page:02}.png", output_prefix.display())),
        ] {
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        Ok(output_prefix.with_extension("png"))
    }

    /// Extract the whole document as plain text.
    pub fn extract_text(&self, pdf: &Path) -> Result<String, PdfError> {
        run_capture(&self.text_command(pdf))
    }
}

fn run(command: &PdfCommand) -> Result<(), PdfError> {
    let status = Command::new(&command.program)
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                PdfError::NotAvailable {
                    missing: vec![program_name(&command.program)],
                }
            } else {
                PdfError::Failed(format!("{}: {error}", command.command_line()))
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(PdfError::Failed(format!(
            "{} exited with {status}",
            command.command_line()
        )))
    }
}

fn run_capture(command: &PdfCommand) -> Result<String, PdfError> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                PdfError::NotAvailable {
                    missing: vec![program_name(&command.program)],
                }
            } else {
                PdfError::Failed(format!("{}: {error}", command.command_line()))
            }
        })?;
    if !output.status.success() {
        return Err(PdfError::Failed(format!(
            "{} exited with {}",
            command.command_line(),
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn program_name(program: &Path) -> String {
    program
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| program.display().to_string())
}

/// Find an executable file called `name` in a `PATH` value.
pub fn find_program(path_var: &str, name: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|directory| !directory.is_empty())
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_bin(tag: &str, programs: &[&str]) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "aether-poppler-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        for program in programs {
            let path = base.join(program);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&path).unwrap().permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&path, permissions).unwrap();
            }
        }
        base
    }

    #[test]
    fn detection_reports_missing_poppler_binaries() {
        let empty = fake_bin("empty", &[]);
        let error = PdfAdapter::detect_in(&empty.display().to_string()).unwrap_err();
        assert!(error.is_not_available());
        assert_eq!(
            error,
            PdfError::NotAvailable {
                missing: vec!["pdftoppm".into(), "pdftotext".into()]
            }
        );
        assert!(error.message().contains("poppler-utils"));
        assert!(!PdfAdapter::detect_in("").is_ok());

        // Only one of the two: the other is named.
        let partial = fake_bin("partial", &["pdftoppm"]);
        let error = PdfAdapter::detect_in(&partial.display().to_string()).unwrap_err();
        assert_eq!(
            error,
            PdfError::NotAvailable {
                missing: vec!["pdftotext".into()]
            }
        );

        let _ = std::fs::remove_dir_all(empty);
        let _ = std::fs::remove_dir_all(partial);
    }

    #[test]
    fn detection_accepts_a_complete_fake_poppler() {
        let bin = fake_bin("full", &["pdftoppm", "pdftotext", "pdfinfo"]);
        let adapter = PdfAdapter::detect_in(&format!(
            "/nonexistent:{}",
            bin.display()
        ))
        .expect("all three programs are on PATH");
        assert!(adapter.pdfinfo.is_some());

        // Without pdfinfo the adapter still works, only the page count is gone.
        let bin2 = fake_bin("no-info", &["pdftoppm", "pdftotext"]);
        let adapter2 = PdfAdapter::detect_in(&bin2.display().to_string()).unwrap();
        assert!(adapter2.pdfinfo.is_none());
        assert!(adapter2
            .page_count_command(Path::new("doc.pdf"))
            .is_none());

        let _ = std::fs::remove_dir_all(bin);
        let _ = std::fs::remove_dir_all(bin2);
    }

    #[test]
    fn render_and_text_commands_are_built_correctly() {
        let bin = fake_bin("commands", &["pdftoppm", "pdftotext", "pdfinfo"]);
        let adapter = PdfAdapter::detect_in(&bin.display().to_string()).unwrap();
        let pdf = Path::new("/docs/report.pdf");
        let prefix = Path::new("/tmp/aether/page");

        let render = adapter.render_command(pdf, 3, prefix, 96);
        assert!(render.program.ends_with("pdftoppm"));
        assert_eq!(
            render.args,
            vec![
                "-png",
                "-r",
                "96",
                "-f",
                "3",
                "-l",
                "3",
                "/docs/report.pdf",
                "/tmp/aether/page"
            ]
        );
        assert!(render.command_line().contains("pdftoppm -png -r 96 -f 3 -l 3"));

        // Page numbers are clamped to 1 (poppler treats 0 as "all pages").
        let first = adapter.render_command(pdf, 0, prefix, 150);
        assert_eq!(first.args[4], "1");
        assert_eq!(first.args[6], "1");
        assert_eq!(first.args[2], "150");

        let text = adapter.text_command(pdf);
        assert!(text.program.ends_with("pdftotext"));
        assert_eq!(text.args, vec!["-layout", "/docs/report.pdf", "-"]);

        let info = adapter.page_count_command(pdf).unwrap();
        assert!(info.program.ends_with("pdfinfo"));
        assert_eq!(info.args, vec!["/docs/report.pdf"]);

        let _ = std::fs::remove_dir_all(bin);
    }
}
