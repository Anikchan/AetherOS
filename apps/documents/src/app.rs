//! Documents controller: the only place that mutates Slint properties
//! (P4-T13, DA-AC-05).
//!
//! Holds the open document, the recent-file list, the current search and the
//! PDF availability note. Tests build it against temporary files and drive it
//! through headless element queries.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ModelRc, VecModel};

use crate::document::{short_path, Document, RecentFiles, SearchHit};
use crate::markdown::Block;
use crate::pdf::PdfAdapter;
use crate::{BlockView, DocEntry, DocumentsApp, HitView};

/// How many match chips fit in the results bar.
const MAX_HIT_CHIPS: usize = 6;

pub struct DocumentsController {
    ui: DocumentsApp,
    document: RefCell<Option<Document>>,
    recent: RefCell<RecentFiles>,
    hits: RefCell<Vec<SearchHit>>,
    query: RefCell<String>,
    pdf_note: String,
    pdf_available: bool,
}

impl DocumentsController {
    pub fn new(ui: DocumentsApp, recent: RecentFiles) -> Rc<DocumentsController> {
        let (pdf_note, pdf_available) = match PdfAdapter::detect() {
            Ok(_) => ("poppler-utils".to_string(), true),
            Err(error) => {
                let note = match &error {
                    crate::pdf::PdfError::NotAvailable { missing } => {
                        format!("Missing {}", missing.join(", "))
                    }
                    other => other.message(),
                };
                (note, false)
            }
        };
        let controller = Rc::new(DocumentsController {
            ui,
            document: RefCell::new(None),
            recent: RefCell::new(recent),
            hits: RefCell::new(Vec::new()),
            query: RefCell::new(String::new()),
            pdf_note,
            pdf_available,
        });
        controller.wire();
        controller.sync();
        controller
    }

    /// Demo content for the screenshot renderer: three real files in a temp
    /// directory (so the recent list opens real documents) and one search so
    /// the results bar is visible.
    pub fn with_demo_content(ui: DocumentsApp, recent: RecentFiles) -> Rc<DocumentsController> {
        let dir = std::env::temp_dir().join("aether-documents-demo");
        let _ = fs::create_dir_all(&dir);
        let primary = dir.join("field-notes.md");
        let design = dir.join("design-review.md");
        let rfc = dir.join("rfc-014-pagination.md");
        let _ = fs::write(&primary, crate::demo::FIELD_NOTES);
        let _ = fs::write(&design, crate::demo::DESIGN_REVIEW);
        let _ = fs::write(&rfc, crate::demo::RFC_PAGINATION);

        let controller = DocumentsController::new(ui, recent);
        {
            let mut recent = controller.recent.borrow_mut();
            recent.push(&design);
            recent.push(&rfc);
            recent.push(&primary);
        }
        controller.open_path(&primary);
        controller.ui.set_find_text("glass".into());
        controller.apply_find("glass");
        controller
    }

    pub fn ui(&self) -> &DocumentsApp {
        &self.ui
    }

    pub fn recent(&self) -> std::cell::Ref<'_, RecentFiles> {
        self.recent.borrow()
    }

    pub fn page_index(&self) -> usize {
        self.document
            .borrow()
            .as_ref()
            .map(Document::page_index)
            .unwrap_or(0)
    }

    pub fn page_count(&self) -> usize {
        self.document
            .borrow()
            .as_ref()
            .map(Document::page_count)
            .unwrap_or(0)
    }

    pub fn hits(&self) -> std::cell::Ref<'_, Vec<SearchHit>> {
        self.hits.borrow()
    }

    pub fn document(&self) -> std::cell::Ref<'_, Option<Document>> {
        self.document.borrow()
    }

    /// Open a document from disk (used by the recent list and by tests).
    pub fn open_path(&self, path: &Path) {
        match Document::open(path) {
            Ok(document) => {
                {
                    let mut recent = self.recent.borrow_mut();
                    recent.push(path);
                    let _ = recent.save();
                }
                *self.document.borrow_mut() = Some(document);
                self.hits.borrow_mut().clear();
                self.query.borrow_mut().clear();
                self.ui.set_find_text("".into());
                self.ui.set_status("".into());
            }
            Err(error) => self.ui.set_status(error.into()),
        }
        self.sync();
    }

    /// Search the open document; the results bar shows the first few matches.
    pub fn apply_find(&self, query: &str) {
        *self.query.borrow_mut() = query.to_string();
        let hits = self
            .document
            .borrow()
            .as_ref()
            .map(|document| document.find(query))
            .unwrap_or_default();
        *self.hits.borrow_mut() = hits;
        self.sync();
    }

    // ------------------------------------------------------------- actions ---

    fn next_page(&self) {
        if let Some(document) = self.document.borrow_mut().as_mut() {
            document.next_page();
        }
        self.sync();
    }

    fn prev_page(&self) {
        if let Some(document) = self.document.borrow_mut().as_mut() {
            document.prev_page();
        }
        self.sync();
    }

    /// Jump to a page (toolbar, page thumbnails and search chips).
    pub fn goto_page(&self, index: usize) {
        if let Some(document) = self.document.borrow_mut().as_mut() {
            document.set_page(index);
        }
        self.sync();
    }

    fn zoom_in(&self) {
        if let Some(document) = self.document.borrow_mut().as_mut() {
            document.zoom_in();
        }
        self.sync();
    }

    fn zoom_out(&self) {
        if let Some(document) = self.document.borrow_mut().as_mut() {
            document.zoom_out();
        }
        self.sync();
    }

    fn wire(self: &Rc<Self>) {
        {
            let weak = Rc::downgrade(self);
            self.ui.on_prev_page(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.prev_page();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_next_page(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.next_page();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_goto_page(move |index| {
                if let Some(controller) = weak.upgrade() {
                    controller.goto_page(index.max(0) as usize);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_zoom_in(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.zoom_in();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_zoom_out(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.zoom_out();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_find(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_find(&controller.ui.get_find_text().to_string());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_find_edited(move |text| {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_find(text.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_open_document(move |path| {
                if let Some(controller) = weak.upgrade() {
                    let path = PathBuf::from(path.as_str());
                    controller.open_path(&path);
                }
            });
        }
    }

    // ---------------------------------------------------------------- sync ---

    pub fn sync(&self) {
        let query = self.query.borrow().clone();
        let hits = self.hits.borrow().clone();
        let document = self.document.borrow();

        self.ui.set_pdf_status(self.pdf_note.clone().into());
        self.ui.set_pdf_available(self.pdf_available);

        let recents: Vec<DocEntry> = self
            .recent
            .borrow()
            .entries()
            .iter()
            .map(|path| DocEntry {
                title: RecentFiles::display_name(path).into(),
                subtitle: path
                    .parent()
                    .map(|parent| short_path(parent))
                    .unwrap_or_default()
                    .into(),
                page: -1,
                path: path.display().to_string().into(),
            })
            .collect();
        self.ui.set_recents(ModelRc::new(VecModel::from(recents)));

        match document.as_ref() {
            Some(document) => {
                self.ui.set_doc_title(document.title().into());
                self.ui
                    .set_doc_path(short_path(document.path()).into());
                self.ui.set_page_label(
                    format!(
                        "Page {} of {}",
                        document.page_number(),
                        document.page_count()
                    )
                    .into(),
                );
                self.ui.set_page_index(document.page_index() as i32);
                self.ui.set_page_count(document.page_count() as i32);
                self.ui
                    .set_zoom_label(format!("{}%", document.zoom_percent()).into());
                self.ui.set_font_scale(document.font_size());

                let blocks: Vec<BlockView> =
                    document.page_blocks().iter().map(block_view).collect();
                self.ui.set_blocks(ModelRc::new(VecModel::from(blocks)));

                let pages: Vec<DocEntry> = (0..document.page_count())
                    .map(|page| {
                        let blocks = &document.pages()[page];
                        // First block with visible text: pages often start on
                        // a divider or an empty line.
                        let subtitle = document.blocks()[blocks.clone()]
                            .iter()
                            .find_map(|block| {
                                let text = block
                                    .text()
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                if text.is_empty() {
                                    return None;
                                }
                                Some(if text.chars().count() > 30 {
                                    format!("{}…", text.chars().take(30).collect::<String>())
                                } else {
                                    text
                                })
                            })
                            .unwrap_or_else(|| "—".to_string());
                        DocEntry {
                            title: format!("Page {}", page + 1).into(),
                            subtitle: subtitle.into(),
                            page: page as i32,
                            path: "".into(),
                        }
                    })
                    .collect();
                self.ui.set_pages(ModelRc::new(VecModel::from(pages)));
            }
            None => {
                self.ui.set_doc_title("No document open".into());
                self.ui.set_doc_path("".into());
                self.ui.set_page_label("Page 0 of 0".into());
                self.ui.set_page_index(0);
                self.ui.set_page_count(0);
                self.ui.set_zoom_label("100%".into());
                self.ui.set_font_scale(1.0);
                self.ui
                    .set_blocks(ModelRc::new(VecModel::from(Vec::<BlockView>::new())));
                self.ui
                    .set_pages(ModelRc::new(VecModel::from(Vec::<DocEntry>::new())));
            }
        }

        let find_label = if query.trim().is_empty() {
            String::new()
        } else {
            format!(
                "{} match{}",
                hits.len(),
                if hits.len() == 1 { "" } else { "es" }
            )
        };
        self.ui.set_find_label(find_label.into());
        // One chip per page: several matches on a page share a jump target.
        let mut pages: Vec<usize> = Vec::new();
        for hit in &hits {
            if !pages.contains(&hit.page) {
                pages.push(hit.page);
            }
        }
        let chips: Vec<HitView> = pages
            .iter()
            .take(MAX_HIT_CHIPS)
            .map(|page| HitView {
                label: format!("p. {}", page + 1).into(),
                preview: hits
                    .iter()
                    .find(|hit| hit.page == *page)
                    .map(|hit| hit.preview.clone())
                    .unwrap_or_default()
                    .into(),
                page: *page as i32,
            })
            .collect();
        self.ui.set_hits(ModelRc::new(VecModel::from(chips)));
    }
}

fn block_view(block: &Block) -> BlockView {
    let marker = match block {
        Block::Bullet { .. } => "•".to_string(),
        Block::Numbered { number, .. } => format!("{number}."),
        Block::Code { language, .. } if !language.is_empty() => language.clone(),
        _ => String::new(),
    };
    let level = match block {
        Block::Heading { level, .. } => *level as i32,
        _ => 0,
    };
    BlockView {
        kind: block.kind().into(),
        text: block.text().into(),
        level,
        marker: marker.into(),
    }
}
