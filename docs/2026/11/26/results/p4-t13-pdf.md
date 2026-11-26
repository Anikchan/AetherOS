# D6 — P4-T13 Documents / PDF viewer (poppler)

- Date: 2026-11-26
- Session: S3
- Task: P4-T13, output `apps/documents`, gate DA-AC-05
- Host: macOS development host; poppler replaced by fake scripts on a temporary `PATH`

## What was verified

| Check | Command | Result |
| --- | --- | --- |
| Documents tests | `cargo test -p aether-documents` | 19 lib, 1 `pdf`, 1 `ui` — all pass |

## Evidence

- `pdfinfo` page count parsing (`Pages: 12`, whitespace, invalid values).
- Zoom → DPI mapping: 100 % = 96 dpi, 25 % clamps to 48, 400 % clamps to 288.
- `PdfDocument`: page count, form-feed page text, page summaries, next/prev
  with clamping, case-insensitive search with page index and preview,
  rendered PNG path exists and starts with the PNG signature.
- End-to-end with fake poppler: open `.pdf` → 3 pages → decoded 4×3 image in
  the surface → search "GLASS" finds page 2 → zoom 125 % re-renders → recent
  list records the file; opening Markdown afterwards clears PDF mode.
- Missing poppler: status shows `Missing pdftoppm, pdftotext (poppler-utils)`,
  no panic, title stays "No document open".
- `pdftoppm` that produces no file is now an explicit error instead of a path
  to a non-existent image.

## Not verified (honest status)

- Rendering a real PDF with the system poppler: poppler is not installed on the
  development host; the flow is exercised with scripted `pdfinfo`/`pdftotext`/
  `pdftoppm` programs. On-device DA-AC-05 remains with the image check.
