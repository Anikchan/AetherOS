//! End-to-end PDF tests for the Documents app (P4-T13, DA-AC-05).
//!
//! poppler is replaced by fake `pdfinfo`/`pdftotext`/`pdftoppm` scripts on a
//! temporary `PATH`, so the whole flow (open → page count → render → decode →
//! search → navigate) runs on any machine. One test function because the Slint
//! testing backend installs a process-wide platform.

use std::path::{Path, PathBuf};

use slint::{ComponentHandle, Model};

use aether_documents::document::RecentFiles;
use aether_documents::pdf::{PdfAdapter, PdfError};
use aether_documents::{DocumentsApp, DocumentsController};

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-documents-pdf-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn write_program(dir: &Path, name: &str, script: &str) {
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

/// Three-page PDF with searchable text; `pdftoppm` copies a 4x3 PNG fixture.
fn fake_poppler(dir: &Path) -> (PdfAdapter, PathBuf) {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fixture = dir.join("fixture.png");
    write_png_fixture(&fixture);
    write_program(&bin, "pdfinfo", "#!/bin/sh\necho 'Pages:          3'\n");
    write_program(
        &bin,
        "pdftotext",
        "#!/bin/sh\nprintf 'Field notes, page one\\fThe glass review, page two\\fAppendix, page three\\f'\n",
    );
    write_program(
        &bin,
        "pdftoppm",
        &format!("#!/bin/sh\ncp '{}' \"$9-$5.png\"\n", fixture.display()),
    );
    let adapter = PdfAdapter::detect_in(&bin.display().to_string()).unwrap();
    (adapter, bin)
}

/// A 4x3 PNG (RGB) written with the same crate the app uses to decode it.
fn write_png_fixture(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 4, 3);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let mut data = Vec::new();
    for y in 0..3u8 {
        for x in 0..4u8 {
            data.extend_from_slice(&[x * 60, y * 80, 200]);
        }
    }
    writer.write_image_data(&data).unwrap();
}

#[test]
fn pdf_viewer_renders_pages_and_searches_text() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = temp_dir("viewer");
    std::fs::create_dir_all(&dir).unwrap();
    let (adapter, bin) = fake_poppler(&dir);
    let pdf_path = dir.join("field-notes.pdf");
    std::fs::write(&pdf_path, b"%PDF-1.4 fake").unwrap();

    let ui = DocumentsApp::new().unwrap();
    ui.show().unwrap();
    let recent = RecentFiles::new(dir.join("recent.txt"));
    let controller = DocumentsController::with_pdf_adapter(ui, recent, Ok(adapter));
    controller.open_path(&pdf_path);

    // The page image is decoded, not a placeholder.
    assert!(controller.is_pdf());
    assert_eq!(controller.ui().get_page_count(), 3);
    assert_eq!(controller.ui().get_page_label().to_string(), "Page 1 of 3");
    assert!(controller.ui().get_is_pdf());
    assert_eq!(controller.ui().get_doc_title().to_string(), "field-notes.pdf");
    let image = controller.ui().get_pdf_page_image();
    assert_eq!(image.size().width, 4);
    assert_eq!(image.size().height, 3);

    // Pages are listed with a text summary.
    assert_eq!(controller.ui().get_pages().row_count(), 3);

    // Search finds the second page and the chip jumps to it.
    controller.apply_find("GLASS");
    assert_eq!(controller.hits().len(), 1);
    assert_eq!(controller.hits()[0].page, 1);
    controller.goto_page(1);
    assert_eq!(controller.ui().get_page_index(), 1);

    // Toolbar navigation renders the next/previous page.
    controller.goto_page(1);
    assert_eq!(controller.ui().get_page_label().to_string(), "Page 2 of 3");
    controller.goto_page(0);
    assert_eq!(controller.ui().get_page_label().to_string(), "Page 1 of 3");

    // Zoom changes the label and re-renders with the new DPI.
    controller.zoom_in();
    assert_eq!(controller.ui().get_zoom_label().to_string(), "125%");

    // The recent list recorded the PDF.
    assert!(controller.recent().entries().contains(&pdf_path));

    // A Markdown file afterwards clears PDF mode.
    let text_path = dir.join("notes.md");
    std::fs::write(&text_path, "# Notes\n\nSome text.\n").unwrap();
    controller.open_path(&text_path);
    assert!(!controller.is_pdf());
    assert!(!controller.ui().get_is_pdf());
    assert_eq!(controller.ui().get_doc_title().to_string(), "notes.md");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&bin).ok();

    // --- a missing poppler explains instead of failing -------------------
    {
        let dir = temp_dir("missing");
        std::fs::create_dir_all(&dir).unwrap();
        let empty_bin = dir.join("bin");
        std::fs::create_dir_all(&empty_bin).unwrap();
        let error = PdfAdapter::detect_in(&empty_bin.display().to_string()).unwrap_err();
        assert!(matches!(error, PdfError::NotAvailable { .. }));

        let ui = DocumentsApp::new().unwrap();
        ui.show().unwrap();
        let recent = RecentFiles::new(dir.join("recent.txt"));
        let controller = DocumentsController::with_pdf_adapter(ui, recent, Err(error));
        let pdf_path = dir.join("report.pdf");
        std::fs::write(&pdf_path, b"%PDF-1.4 fake").unwrap();
        controller.open_path(&pdf_path);

        assert!(!controller.ui().get_pdf_available());
        assert!(!controller.is_pdf());
        assert!(controller.ui().get_status().to_string().contains("poppler-utils"));
        assert_eq!(controller.ui().get_doc_title().to_string(), "No document open");

        std::fs::remove_dir_all(&dir).ok();
    }
}
