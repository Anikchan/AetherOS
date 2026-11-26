//! PDF adapter for the Documents app (P4-T13, DA-AC-05).
//!
//! AetherOS keeps no PDF engine of its own in the base image: the viewer drives
//! `poppler-utils` when it is installed (`pdftoppm` renders a page to PNG,
//! `pdftotext` extracts text, `pdfinfo` reports the page count). When the
//! binaries are missing the adapter returns [`PdfError::NotAvailable`] with the
//! list of missing programs — the viewer shows "PDF preview needs poppler-utils"
//! instead of panicking or crashing.
//!
//! On top of the raw commands, [`PdfDocument`] is the viewer model: page count
//! (from `pdfinfo`, falling back to the form-feed count from `pdftotext`), page
//! navigation, zoom → render DPI, per-page text for search, and the cache path
//! a rendered page is written to.
//!
//! The adapter only builds and runs commands; availability detection, the
//! command lines and the whole `PdfDocument` flow are unit-tested against a
//! fabricated `PATH` with fake poppler scripts, so the tests do not need
//! poppler installed.

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
        Err(PdfError::Failed(format!(
            "{} produced no image for page {page}",
            self.pdftoppm.display()
        )))
    }

    /// Extract the whole document as plain text.
    pub fn extract_text(&self, pdf: &Path) -> Result<String, PdfError> {
        run_capture(&self.text_command(pdf))
    }

    /// Page count from `pdfinfo`; `None` when `pdfinfo` is missing or its
    /// output does not contain a `Pages:` line.
    pub fn page_count(&self, pdf: &Path) -> Option<u32> {
        let command = self.page_count_command(pdf)?;
        let output = run_capture(&command).ok()?;
        parse_page_count(&output)
    }
}

/// Parse the `Pages:` line of `pdfinfo` output.
pub fn parse_page_count(pdfinfo: &str) -> Option<u32> {
    pdfinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !key.trim().eq_ignore_ascii_case("pages") {
            return None;
        }
        value.trim().parse::<u32>().ok().filter(|pages| *pages > 0)
    })
}

/// Render DPI for a zoom percentage: 96 dpi is 100 %, clamped to 48..288 dpi
/// so neither a thumbnail nor a 400 % zoom can ask poppler for a huge bitmap.
pub fn dpi_for_zoom(zoom_percent: u32) -> u32 {
    (96 * zoom_percent as u64 / 100).clamp(48, 288) as u32
}

/// Minimum and maximum zoom for the PDF toolbar.
pub const MIN_PDF_ZOOM: u32 = 25;
pub const MAX_PDF_ZOOM: u32 = 400;
pub const PDF_ZOOM_STEP: u32 = 25;

/// An opened PDF: page count, text per page, page/zoom state and a cache
/// directory for rendered PNGs.
#[derive(Debug, Clone)]
pub struct PdfDocument {
    path: PathBuf,
    adapter: PdfAdapter,
    texts: Vec<String>,
    cache_dir: PathBuf,
    pages: u32,
    page: u32,
    zoom_percent: u32,
}

impl PdfDocument {
    /// Open a PDF. Text extraction is best effort (a scanned PDF has none) and
    /// the page count falls back to the number of form-feed separated pages.
    pub fn open(
        adapter: PdfAdapter,
        path: impl AsRef<Path>,
        cache_dir: impl Into<PathBuf>,
    ) -> Result<PdfDocument, PdfError> {
        let path = path.as_ref().to_path_buf();
        let text = adapter.extract_text(&path).unwrap_or_default();
        let mut texts: Vec<String> = text
            .split('\u{0c}')
            .map(|page| page.trim_matches(['\r', '\n']).to_string())
            .collect();
        while texts.last().map(|page| page.is_empty()).unwrap_or(false) {
            texts.pop();
        }
        if texts.is_empty() {
            texts.push(String::new());
        }
        let pages = adapter
            .page_count(&path)
            .unwrap_or(texts.len() as u32)
            .max(1);
        while texts.len() < pages as usize {
            texts.push(String::new());
        }
        let cache_dir = cache_dir.into();
        std::fs::create_dir_all(&cache_dir)
            .map_err(|error| PdfError::Failed(format!("cannot create cache: {error}")))?;
        Ok(PdfDocument {
            path,
            adapter,
            texts,
            cache_dir,
            pages,
            page: 0,
            zoom_percent: 100,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn page_count(&self) -> u32 {
        self.pages
    }

    /// Zero-based page index.
    pub fn page_index(&self) -> u32 {
        self.page
    }

    /// One-based page number for the toolbar.
    pub fn page_number(&self) -> u32 {
        self.page + 1
    }

    pub fn zoom_percent(&self) -> u32 {
        self.zoom_percent
    }

    pub fn dpi(&self) -> u32 {
        dpi_for_zoom(self.zoom_percent)
    }

    pub fn page_text(&self) -> &str {
        self.texts
            .get(self.page as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Short first line for the page list in the sidebar.
    pub fn page_summary(&self, index: u32) -> String {
        self.texts
            .get(index as usize)
            .and_then(|text| text.lines().find(|line| !line.trim().is_empty()))
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.chars().count() > 34 {
                    format!("{}…", trimmed.chars().take(34).collect::<String>())
                } else {
                    trimmed.to_string()
                }
            })
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn set_page(&mut self, index: u32) {
        self.page = index.min(self.pages.saturating_sub(1));
    }

    pub fn next_page(&mut self) -> bool {
        if self.page + 1 < self.pages {
            self.page += 1;
            true
        } else {
            false
        }
    }

    pub fn prev_page(&mut self) -> bool {
        if self.page > 0 {
            self.page -= 1;
            true
        } else {
            false
        }
    }

    pub fn zoom_in(&mut self) -> bool {
        if self.zoom_percent + PDF_ZOOM_STEP <= MAX_PDF_ZOOM {
            self.zoom_percent += PDF_ZOOM_STEP;
            true
        } else {
            false
        }
    }

    pub fn zoom_out(&mut self) -> bool {
        if self.zoom_percent > MIN_PDF_ZOOM {
            self.zoom_percent = (self.zoom_percent - PDF_ZOOM_STEP).max(MIN_PDF_ZOOM);
            true
        } else {
            false
        }
    }

    /// Cache prefix for the current page at the current zoom.
    pub fn page_prefix(&self) -> PathBuf {
        self.cache_dir
            .join(format!("p{}-{}dpi", self.page, self.dpi()))
    }

    /// Render the current page and return the produced PNG path.
    pub fn render(&self) -> Result<PathBuf, PdfError> {
        self.adapter
            .render_page(&self.path, self.page + 1, &self.page_prefix(), self.dpi())
    }

    /// Case-insensitive search over the extracted text, one hit per page.
    pub fn find(&self, query: &str) -> Vec<PageHit> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.texts
            .iter()
            .enumerate()
            .filter_map(|(page, text)| {
                let haystack = text.to_lowercase();
                let offset = haystack.find(&needle)?;
                let start = haystack[..offset]
                    .char_indices()
                    .rev()
                    .nth(40)
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                let end = (offset + needle.len() + 40).min(haystack.len());
                let preview = text
                    .get(start..end)
                    .unwrap_or(&text[..])
                    .replace(['\n', '\r', '\u{0c}'], " ");
                Some(PageHit {
                    page: page as u32,
                    preview: preview.trim().to_string(),
                })
            })
            .collect()
    }
}

/// One search result inside a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHit {
    /// Zero-based page index.
    pub page: u32,
    pub preview: String,
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

    fn fake_program(dir: &Path, name: &str, script: &str) {
        let path = dir.join(name);
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
    }

    /// poppler stubs: three pages, text separated by form feeds, and a copy of
    /// a PNG fixture for every rendered page.
    fn fake_poppler(tag: &str, fixture: &Path) -> (PdfAdapter, PathBuf) {
        let bin = fake_bin(tag, &[]);
        fake_program(&bin, "pdfinfo", "#!/bin/sh\necho 'Pages:          3'\n");
        fake_program(
            &bin,
            "pdftotext",
            "#!/bin/sh\nprintf 'Page one\\fPage two has glass\\fPage three\\f'\n",
        );
        fake_program(
            &bin,
            "pdftoppm",
            &format!("#!/bin/sh\ncp '{}' \"$9-$5.png\"\n", fixture.display()),
        );
        let adapter = PdfAdapter::detect_in(&bin.display().to_string()).unwrap();
        (adapter, bin)
    }

    /// 2x2 opaque PNG written by the `png` crate (already a Documents dep for
    /// showing rendered pages).
    fn png_fixture(path: &Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            .unwrap();
    }

    #[test]
    fn page_count_parses_pdfinfo_output() {
        assert_eq!(
            parse_page_count("Title: report\nPages:          12\nEncrypted: no"),
            Some(12)
        );
        assert_eq!(parse_page_count("pages: 7"), Some(7));
        assert_eq!(parse_page_count("Pages:           0"), None);
        assert_eq!(parse_page_count("Title: no page count"), None);
        assert_eq!(parse_page_count("Pages: not a number"), None);
    }

    #[test]
    fn dpi_for_zoom_covers_the_toolbar_range() {
        assert_eq!(dpi_for_zoom(100), 96);
        assert_eq!(dpi_for_zoom(50), 48);
        assert_eq!(dpi_for_zoom(400), 288); // 384 clamped
        assert_eq!(dpi_for_zoom(25), 48); // 24 clamped up
        assert_eq!(dpi_for_zoom(200), 192);
    }

    #[test]
    fn pdf_document_pages_text_and_navigation() {
        let dir = std::env::temp_dir().join(format!(
            "aether-pdfdoc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = dir.join("page.png");
        png_fixture(&fixture);
        let (adapter, bin) = fake_poppler("doc", &fixture);
        let pdf_path = dir.join("report.pdf");
        std::fs::write(&pdf_path, b"%PDF-1.4 fake").unwrap();

        let mut document = PdfDocument::open(adapter, &pdf_path, dir.join("cache")).unwrap();
        assert_eq!(document.page_count(), 3);
        assert_eq!(document.page_number(), 1);
        assert_eq!(document.page_text(), "Page one");
        assert_eq!(document.zoom_percent(), 100);
        assert_eq!(document.dpi(), 96);

        assert!(document.next_page());
        assert_eq!(document.page_number(), 2);
        assert!(document.page_text().contains("glass"));
        assert_eq!(document.page_summary(1), "Page two has glass");
        assert!(document.next_page(), "page 3 exists");
        assert!(!document.next_page(), "no page past the end");
        assert_eq!(document.page_number(), 3);
        assert!(document.prev_page());
        assert_eq!(document.page_number(), 2);

        // Rendering writes the PNG the caller decodes.
        let rendered = document.render().unwrap();
        assert!(rendered.exists(), "fake pdftoppm wrote {rendered:?}");
        let bytes = std::fs::read(&rendered).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

        // Zoom changes the requested DPI and the cache prefix.
        assert!(document.zoom_in());
        assert_eq!(document.zoom_percent(), 125);
        assert_eq!(document.dpi(), 120);
        assert!(document.page_prefix().to_string_lossy().contains("120dpi"));
        for _ in 0..40 {
            document.zoom_in();
        }
        assert_eq!(document.zoom_percent(), MAX_PDF_ZOOM);
        for _ in 0..40 {
            document.zoom_out();
        }
        assert_eq!(document.zoom_percent(), MIN_PDF_ZOOM);

        // Search is case-insensitive and reports the zero-based page.
        let hits = document.find("GLASS");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page, 1);
        assert!(hits[0].preview.contains("glass"));
        assert!(document.find("   ").is_empty());
        assert!(document.find("absent").is_empty());

        // Jumping past the end clamps.
        document.set_page(99);
        assert_eq!(document.page_number(), 3);

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(bin);
    }

    #[test]
    fn pdf_document_survives_missing_text_and_page_count() {
        let dir = std::env::temp_dir().join(format!(
            "aether-pdfdoc-notext-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = fake_bin("notext", &["pdftoppm", "pdftotext"]);
        // pdftotext fails (scanned PDF) and pdfinfo is absent: one page, no text.
        fake_program(&bin, "pdftotext", "#!/bin/sh\nexit 1\n");
        let adapter = PdfAdapter::detect_in(&bin.display().to_string()).unwrap();
        let pdf = dir.join("scan.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 fake").unwrap();

        let document = PdfDocument::open(adapter, &pdf, dir.join("cache")).unwrap();
        assert_eq!(document.page_count(), 1);
        assert_eq!(document.page_text(), "");
        assert_eq!(document.page_summary(0), "—");
        assert!(document.find("anything").is_empty());

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(bin);
    }
}
