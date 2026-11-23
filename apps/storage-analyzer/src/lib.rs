//! Storage analyzer (Pack G, P7-PK-T04, F15).
//!
//! Scans a directory tree once, reports per-directory usage and the largest
//! files, and can compare its total with `du -sb` output (gate: within 1%).
//! Reads only; nothing is deleted.

use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileUsage {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirUsage {
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScanReport {
    pub root: PathBuf,
    pub total_bytes: u64,
    pub total_files: u64,
    pub total_dirs: u64,
    pub top_files: Vec<FileUsage>,
    pub dirs: Vec<DirUsage>,
    pub errors: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("path {0} does not exist")]
    NotFound(PathBuf),
    #[error("{0} is not a directory")]
    NotDirectory(PathBuf),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Scan `root`, keeping the `top` largest files and per-directory totals.
pub fn scan(root: &Path, top: usize) -> Result<ScanReport, ScanError> {
    if !root.exists() {
        return Err(ScanError::NotFound(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(ScanError::NotDirectory(root.to_path_buf()));
    }
    let mut report = ScanReport {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let mut directories: BTreeMap<PathBuf, (u64, u64)> = BTreeMap::new();
    let mut files: Vec<FileUsage> = Vec::new();

    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.errors.push(error.to_string());
                continue;
            }
        };
        if entry.file_type().is_dir() {
            report.total_dirs += 1;
            directories.entry(entry.path().to_path_buf()).or_insert((0, 0));
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let size = match entry.metadata() {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                report.errors.push(error.to_string());
                continue;
            }
        };
        report.total_files += 1;
        report.total_bytes += size;
        files.push(FileUsage {
            path: entry.path().to_path_buf(),
            bytes: size,
        });
        // Charge the file to every ancestor directory inside the root.
        let mut cursor = entry.path().parent();
        while let Some(dir) = cursor {
            if !dir.starts_with(root) {
                break;
            }
            let record = directories.entry(dir.to_path_buf()).or_insert((0, 0));
            record.0 += size;
            record.1 += 1;
            cursor = dir.parent();
        }
    }

    files.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    files.truncate(top.max(1));
    report.top_files = files;

    let mut dirs: Vec<DirUsage> = directories
        .into_iter()
        .map(|(path, (bytes, files))| DirUsage { path, bytes, files })
        .collect();
    dirs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    report.dirs = dirs;
    Ok(report)
}

/// Parse `du -sb PATH` output (`12345\tPATH`) and compare with a scan total.
pub fn compare_with_du(report: &ScanReport, du_output: &str) -> Option<f64> {
    let du_bytes: u64 = du_output
        .lines()
        .next()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    if du_bytes == 0 {
        return None;
    }
    let diff = (report.total_bytes as f64 - du_bytes as f64).abs();
    Some(diff / du_bytes as f64 * 100.0)
}

/// Print a du-style map of the largest directories.
pub fn render_map(report: &ScanReport, limit: usize) -> String {
    let mut out = format!(
        "Storage analyzer — {}\nTotal: {}\nFiles: {}  Directories: {}\n\n",
        report.root.display(),
        human_size(report.total_bytes),
        report.total_files,
        report.total_dirs
    );
    out.push_str("Largest directories:\n");
    for dir in report.dirs.iter().take(limit) {
        out.push_str(&format!(
            "  {:>10}  {} ({})\n",
            human_size(dir.bytes),
            dir.path.display(),
            dir.files
        ));
    }
    out.push_str("\nLargest files:\n");
    for file in &report.top_files {
        out.push_str(&format!(
            "  {:>10}  {}\n",
            human_size(file.bytes),
            file.path.display()
        ));
    }
    if !report.errors.is_empty() {
        out.push_str(&format!("\nSkipped ({}):\n", report.errors.len()));
        for error in report.errors.iter().take(10) {
            out.push_str(&format!("  - {error}\n"));
        }
    }
    out
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Suggest cleanup candidates (files over `min_bytes`), never deleting.
pub fn large_files(report: &ScanReport, min_bytes: u64) -> Vec<&FileUsage> {
    report
        .top_files
        .iter()
        .filter(|file| file.bytes >= min_bytes)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-storage-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, size: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(&vec![0u8; size]).unwrap();
    }

    #[test]
    fn scan_totals_and_largest_files() {
        let root = fixture("scan");
        write(&root.join("a.txt"), 100);
        write(&root.join("sub/b.bin"), 500);
        write(&root.join("sub/deep/c.bin"), 1000);
        let report = scan(&root, 2).unwrap();
        assert_eq!(report.total_files, 3);
        assert_eq!(report.total_bytes, 1600);
        assert_eq!(report.top_files.len(), 2);
        assert_eq!(report.top_files[0].bytes, 1000);
        assert_eq!(report.dirs[0].path, root);
        assert_eq!(report.dirs[0].bytes, 1600);
        assert!(report
            .dirs
            .iter()
            .any(|dir| dir.path == root.join("sub") && dir.bytes == 1500));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn du_comparison_is_within_one_percent() {
        let root = fixture("du");
        write(&root.join("x"), 1000);
        let report = scan(&root, 1).unwrap();
        let deviation = compare_with_du(&report, &format!("1000\t{}", root.display())).unwrap();
        assert!(deviation <= 1.0);
        let deviation = compare_with_du(&report, &format!("900\t{}", root.display())).unwrap();
        assert!(deviation > 1.0);
    }

    #[test]
    fn missing_paths_are_reported() {
        assert!(matches!(
            scan(Path::new("/definitely/not/here"), 10),
            Err(ScanError::NotFound(_))
        ));
    }

    #[test]
    fn map_and_large_files_render() {
        let root = fixture("map");
        write(&root.join("big.iso"), 2048);
        let report = scan(&root, 5).unwrap();
        let map = render_map(&report, 5);
        assert!(map.contains("Storage analyzer"));
        assert!(map.contains("big.iso"));
        assert_eq!(large_files(&report, 1024).len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn symlinks_are_not_followed() {
        let root = fixture("links");
        write(&root.join("dir/file"), 10);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("dir"), root.join("loop")).unwrap();
        }
        let report = scan(&root, 5).unwrap();
        assert_eq!(report.total_files, 1);
        let _ = fs::remove_dir_all(root);
    }
}
