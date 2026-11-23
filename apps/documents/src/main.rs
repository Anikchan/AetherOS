//! `aether-documents` entry point (P4-T13, DA-AC-05).
//!
//! ```text
//! aether-documents --render <dir>    render app-documents.png into <dir>
//! aether-documents --info <file>     print block/page/search summary
//! aether-documents --help
//! ```

use std::path::PathBuf;

use aether_documents::document::{Document, RecentFiles};
use aether_documents::{DocumentsApp, DocumentsController};

fn usage() {
    println!(
        "aether-documents — AetherOS Documents / PDF viewer (P4-T13)\n\
         \n\
         Usage:\n\
         \x20 aether-documents --render <dir>  render app-documents.png into <dir>\n\
         \x20 aether-documents --info <file>   parse a file and print its structure\n\
         \x20 aether-documents --help          this message"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--render") => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("screenshots"));
            aether_ui::render::init()?;
            let ui = DocumentsApp::new()?;
            let recent = RecentFiles::new(
                std::env::temp_dir()
                    .join("aether-documents-render")
                    .join("recent.txt"),
            );
            let controller = DocumentsController::with_demo_content(ui, recent);
            let path = dir.join("app-documents.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_DOCUMENTS_RENDERED {} -> {} ({}x{})",
                dir.display(),
                path.display(),
                frame.width,
                frame.height
            );

            // Second page shows the code block, quote and divider styling.
            controller.goto_page(1);
            let page_two = dir.join("app-documents-page2.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &page_two)?;
            println!(
                "AETHER_APP_DOCUMENTS_PAGE2_RENDERED {} -> {} ({}x{})",
                dir.display(),
                page_two.display(),
                frame.width,
                frame.height
            );
        }
        Some("--info") => {
            let path = args.get(1).ok_or("--info needs a file path")?;
            let document = Document::open(path)?;
            println!("title: {}", document.title());
            println!("blocks: {}", document.blocks().len());
            println!("pages: {}", document.page_count());
            println!("page-lines: {}", document.lines_per_page());
            println!("zoom: {}%", document.zoom_percent());
            for (index, page) in document.pages().iter().enumerate() {
                println!("page {}: blocks {}..{}", index + 1, page.start, page.end);
            }
            let hits = document.find("the");
            println!("matches for \"the\": {}", hits.len());
        }
        Some("--help") | None => usage(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}
