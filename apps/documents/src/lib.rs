//! AetherOS Documents default app (P4-T13, DA-AC-05).
//!
//! * [`markdown`] — line-based Markdown parser (blocks + inline spans).
//! * [`document`] — open/paginate/zoom/search a document, recent files.
//! * [`pdf`] — poppler-utils adapter with a clear `NotAvailable` error.
//! * [`app`] — `DocumentsController`, the only place that mutates Slint
//!   properties.
//! * [`demo`] — sample Markdown for screenshots and tests.

slint::include_modules!();

pub mod app;
pub mod demo;
pub mod document;
pub mod markdown;
pub mod pdf;

pub use app::DocumentsController;
pub use document::{Document, RecentFiles, SearchHit};
pub use markdown::{parse, Block, Span};
pub use pdf::{PdfAdapter, PdfError};
