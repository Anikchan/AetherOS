//! Archive formats for the Archive app (P4-T17, DA-AC-08).
//!
//! Supports TAR, TAR.GZ (TGZ) and ZIP: format detection by extension and by
//! magic bytes, entry listing, extraction of everything or a selection,
//! tar.gz creation and rebuild-based deletion. Extraction refuses absolute
//! paths, any `..` component (zip-slip / tar-slip) and any member that would
//! traverse a symlink already present in the destination. The name guard runs
//! per member before that member is written, so extraction is not transactional:
//! members that came before a refused one stay on disk. All tests build archives
//! at runtime in temporary directories.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Tar,
    TarGz,
    Zip,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Tar => "TAR",
            Format::TarGz => "TAR.GZ",
            Format::Zip => "ZIP",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            Some(Format::TarGz)
        } else if lower.ends_with(".tar") {
            Some(Format::Tar)
        } else if lower.ends_with(".zip") {
            Some(Format::Zip)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub enum ArchiveError {
    UnsupportedFormat(PathBuf),
    /// An entry name would escape the destination folder.
    MaliciousEntry(String),
    InvalidName(String),
    Io { path: PathBuf, source: io::Error },
    Codec(String),
}

impl ArchiveError {
    fn io(path: &Path, source: io::Error) -> Self {
        ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::UnsupportedFormat(path) => {
                write!(f, "unsupported archive format: {}", path.display())
            }
            ArchiveError::MaliciousEntry(name) => {
                write!(f, "refused unsafe archive entry: {name}")
            }
            ArchiveError::InvalidName(name) => write!(f, "invalid name: {name}"),
            ArchiveError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            ArchiveError::Codec(message) => write!(f, "archive error: {message}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(error: zip::result::ZipError) -> Self {
        match error {
            zip::result::ZipError::Io(source) => ArchiveError::Io {
                path: PathBuf::new(),
                source,
            },
            other => ArchiveError::Codec(other.to_string()),
        }
    }
}

/// One member of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub modified: Option<SystemTime>,
    pub mode: u32,
}

impl Entry {
    pub fn kind_label(&self) -> &'static str {
        if self.is_dir {
            "Folder"
        } else {
            "File"
        }
    }

    pub fn size_label(&self) -> String {
        if self.is_dir {
            "—".to_string()
        } else {
            format_bytes(self.size)
        }
    }
}

/// Detect the format from the magic bytes first, then the name.
pub fn detect_format(path: &Path) -> Result<Format, ArchiveError> {
    if let Ok(mut file) = File::open(path) {
        let mut magic = [0_u8; 512];
        let read = file.read(&mut magic).unwrap_or(0);
        if read >= 4 && &magic[..4] == b"PK\x03\x04" {
            return Ok(Format::Zip);
        }
        if read >= 2 && magic[..2] == [0x1f, 0x8b] {
            return Ok(Format::TarGz);
        }
        if read >= 262 && &magic[257..262] == b"ustar" {
            return Ok(Format::Tar);
        }
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Format::from_name(&name).ok_or_else(|| ArchiveError::UnsupportedFormat(path.to_path_buf()))
}

/// Reject absolute paths, drive prefixes and any `..` component.
pub fn safe_entry_path(name: &str) -> Result<PathBuf, ArchiveError> {
    let path = Path::new(name);
    if path.is_absolute() {
        return Err(ArchiveError::MaliciousEntry(name.to_string()));
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ArchiveError::MaliciousEntry(name.to_string()));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(ArchiveError::MaliciousEntry(name.to_string()));
    }
    Ok(relative)
}

fn normalize_name(name: &str) -> String {
    name.trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

/// List every member without writing anything.
pub fn list(path: &Path) -> Result<(Format, Vec<Entry>), ArchiveError> {
    let format = detect_format(path)?;
    let entries = match format {
        Format::Zip => list_zip(path)?,
        Format::Tar | Format::TarGz => list_tar(path, format)?,
    };
    Ok((format, entries))
}

fn list_tar(path: &Path, format: Format) -> Result<Vec<Entry>, ArchiveError> {
    let reader = open_reader(path, format)?;
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    let iterator = archive
        .entries()
        .map_err(|error| ArchiveError::io(path, error))?;
    for item in iterator {
        let item = item.map_err(|error| ArchiveError::io(path, error))?;
        let header = item.header();
        let path_name = String::from_utf8_lossy(&item.path_bytes()).into_owned();
        let is_dir = header.entry_type().is_dir();
        let size = item.size();
        let modified = header
            .mtime()
            .ok()
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)));
        entries.push(Entry {
            path: path_name,
            size,
            is_dir,
            modified,
            mode: header.mode().unwrap_or(0o644),
        });
    }
    Ok(entries)
}

fn list_zip(path: &Path) -> Result<Vec<Entry>, ArchiveError> {
    let file = File::open(path).map_err(|error| ArchiveError::io(path, error))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let item = archive.by_index(index)?;
        entries.push(Entry {
            path: item.name().to_string(),
            size: item.size(),
            is_dir: item.is_dir(),
            modified: item.last_modified().map(system_time_from_zip),
            mode: item.unix_mode().unwrap_or(0o644),
        });
    }
    Ok(entries)
}

/// Extract every member into `dest`.
pub fn extract_all(
    archive: &Path,
    dest: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    extract_impl(archive, dest, None, progress)
}

/// Extract only the named members into `dest`.
pub fn extract_selected(
    archive: &Path,
    dest: &Path,
    selected: &[String],
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    extract_impl(archive, dest, Some(selected), progress)
}

fn extract_impl(
    archive: &Path,
    dest: &Path,
    selection: Option<&[String]>,
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    let format = detect_format(archive)?;
    fs::create_dir_all(dest).map_err(|error| ArchiveError::io(dest, error))?;
    let wanted: Option<Vec<String>> =
        selection.map(|names| names.iter().map(|name| normalize_name(name)).collect());
    match format {
        Format::Zip => extract_zip(archive, dest, wanted.as_deref(), progress),
        Format::Tar | Format::TarGz => extract_tar(archive, dest, format, wanted.as_deref(), progress),
    }
}

fn extract_tar(
    archive: &Path,
    dest: &Path,
    format: Format,
    wanted: Option<&[String]>,
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    let total = list(&archive.to_path_buf())?
        .1
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.size)
        .sum::<u64>();
    let reader = open_reader(archive, format)?;
    let mut tar_archive = tar::Archive::new(reader);
    let mut done = 0_u64;
    let mut extracted = 0_usize;
    let entries = tar_archive
        .entries()
        .map_err(|error| ArchiveError::io(archive, error))?;
    for item in entries {
        let mut item = item.map_err(|error| ArchiveError::io(archive, error))?;
        let raw = String::from_utf8_lossy(&item.path_bytes()).into_owned();
        let relative = safe_entry_path(&raw)?;
        if let Some(wanted) = wanted {
            let normalized = normalize_name(&raw);
            if !wanted
                .iter()
                .any(|name| *name == normalized || normalized.starts_with(&format!("{name}/")))
            {
                continue;
            }
        }
        let target = prepare_target(dest, &relative, &raw)?;
        let header = item.header();
        let is_dir = header.entry_type().is_dir();
        let mode = header.mode().unwrap_or(0o644);
        if is_dir {
            fs::create_dir_all(&target).map_err(|error| ArchiveError::io(&target, error))?;
            extracted += 1;
            continue;
        }
        // Only regular files are materialised; links are skipped so an archive
        // cannot create a link that later escapes the destination.
        if !header.entry_type().is_file() {
            continue;
        }
        let mut file = File::create(&target).map_err(|error| ArchiveError::io(&target, error))?;
        io::copy(&mut item, &mut file).map_err(|error| ArchiveError::io(&target, error))?;
        apply_mode(&target, mode);
        done += item.size();
        progress(done, total);
        extracted += 1;
    }
    Ok(extracted)
}

fn extract_zip(
    archive: &Path,
    dest: &Path,
    wanted: Option<&[String]>,
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    let total: u64 = list_zip(archive)?
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| entry.size)
        .sum();
    let file = File::open(archive).map_err(|error| ArchiveError::io(archive, error))?;
    let mut zip_archive = zip::ZipArchive::new(file)?;
    let mut done = 0_u64;
    let mut extracted = 0_usize;
    for index in 0..zip_archive.len() {
        let mut item = zip_archive.by_index(index)?;
        let raw = item.name().to_string();
        let relative = safe_entry_path(&raw)?;
        if let Some(wanted) = wanted {
            let normalized = normalize_name(&raw);
            if !wanted
                .iter()
                .any(|name| *name == normalized || normalized.starts_with(&format!("{name}/")))
            {
                continue;
            }
        }
        let target = prepare_target(dest, &relative, &raw)?;
        if item.is_dir() {
            fs::create_dir_all(&target).map_err(|error| ArchiveError::io(&target, error))?;
            extracted += 1;
            continue;
        }
        let mut out = File::create(&target).map_err(|error| ArchiveError::io(&target, error))?;
        io::copy(&mut item, &mut out).map_err(|error| ArchiveError::io(&target, error))?;
        apply_mode(&target, item.unix_mode().unwrap_or(0o644));
        done += item.size();
        progress(done, total);
        extracted += 1;
    }
    Ok(extracted)
}

/// Resolve a member target inside `dest` without ever following a symlink.
///
/// `safe_entry_path` only validates the member *name*. This walks the parent
/// components from the canonical destination down, refuses any existing symlink
/// or non-directory component, creates missing directories, refuses a final
/// component that is already a symlink (writing through it would escape), and
/// confirms the result is still inside the destination.
fn prepare_target(dest: &Path, relative: &Path, entry: &str) -> Result<PathBuf, ArchiveError> {
    fs::create_dir_all(dest).map_err(|error| ArchiveError::io(dest, error))?;
    let dest_canon = fs::canonicalize(dest).map_err(|error| ArchiveError::io(dest, error))?;
    let target = dest_canon.join(relative);
    if !target.starts_with(&dest_canon) {
        return Err(ArchiveError::MaliciousEntry(entry.to_string()));
    }

    let mut current = dest_canon.clone();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(ArchiveError::MaliciousEntry(entry.to_string()));
                }
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(ArchiveError::io(
                        &current,
                        io::Error::new(
                            io::ErrorKind::Other,
                            "path component is not a directory",
                        ),
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&current).map_err(|error| ArchiveError::io(&current, error))?;
                }
                Err(error) => return Err(ArchiveError::io(&current, error)),
            }
        }
    }

    if let Ok(meta) = fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            return Err(ArchiveError::MaliciousEntry(entry.to_string()));
        }
    }
    Ok(target)
}

/// Write a tar.gz containing `sources` (files or folders, by name).
pub fn create_tar_gz(
    archive: &Path,
    sources: &[PathBuf],
    progress: &mut impl FnMut(u64, u64),
) -> Result<u64, ArchiveError> {
    if sources.is_empty() {
        return Err(ArchiveError::InvalidName("nothing to archive".into()));
    }
    let total: u64 = sources
        .iter()
        .map(|source| tree_size(source).unwrap_or(0))
        .sum();
    let file = File::create(archive).map_err(|error| ArchiveError::io(archive, error))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let mut done = 0_u64;
    for source in sources {
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| ArchiveError::InvalidName(source.display().to_string()))?;
        if source.is_dir() {
            builder
                .append_dir_all(&name, source)
                .map_err(|error| ArchiveError::io(source, error))?;
        } else {
            builder
                .append_path_with_name(source, &name)
                .map_err(|error| ArchiveError::io(source, error))?;
        }
        done += tree_size(source).unwrap_or(0);
        progress(done, total);
    }
    let encoder = builder
        .into_inner()
        .map_err(|error| ArchiveError::io(archive, error))?;
    encoder
        .finish()
        .map_err(|error| ArchiveError::io(archive, error))?;
    Ok(done)
}

/// Remove members by rebuilding the archive next to the original and swapping
/// it into place. Returns the number of members left.
pub fn delete_entries(
    archive: &Path,
    names: &[String],
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    let format = detect_format(archive)?;
    let doomed: Vec<String> = names.iter().map(|name| normalize_name(name)).collect();
    // Selecting a folder deletes everything inside it, not only the folder
    // entry itself.
    let keep = |name: &str| {
        let normalized = normalize_name(name);
        !doomed.iter().any(|entry| {
            normalized == *entry || normalized.starts_with(&format!("{entry}/"))
        })
    };
    let (_, entries) = list(archive)?;
    let total: u64 = entries
        .iter()
        .filter(|entry| !entry.is_dir && keep(&entry.path))
        .map(|entry| entry.size)
        .sum();

    let temp = archive.with_extension("rebuild");
    let result = match format {
        Format::Tar => {
            let reader = open_reader(archive, Format::Tar)?;
            let out = File::create(&temp).map_err(|error| ArchiveError::io(&temp, error))?;
            let mut builder = tar::Builder::new(out);
            let copied = copy_tar_entries(reader, &mut builder, &keep, total, progress)?;
            builder
                .into_inner()
                .map_err(|error| ArchiveError::io(&temp, error))?;
            Ok(copied)
        }
        Format::TarGz => {
            let reader = open_reader(archive, Format::TarGz)?;
            let out = File::create(&temp).map_err(|error| ArchiveError::io(&temp, error))?;
            let encoder = GzEncoder::new(out, Compression::default());
            let mut builder = tar::Builder::new(encoder);
            let copied = copy_tar_entries(reader, &mut builder, &keep, total, progress)?;
            let encoder = builder
                .into_inner()
                .map_err(|error| ArchiveError::io(&temp, error))?;
            encoder
                .finish()
                .map_err(|error| ArchiveError::io(&temp, error))?;
            Ok(copied)
        }
        Format::Zip => rebuild_zip(archive, &temp, &keep, total, progress),
    };
    let copied = result?;
    fs::rename(&temp, archive).map_err(|error| ArchiveError::io(archive, error))?;
    Ok(copied)
}

fn copy_tar_entries(
    reader: Box<dyn Read>,
    builder: &mut tar::Builder<impl Write>,
    keep: &dyn Fn(&str) -> bool,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    let mut archive = tar::Archive::new(reader);
    let mut copied = 0_usize;
    let mut done = 0_u64;
    let entries = archive
        .entries()
        .map_err(|error| ArchiveError::Codec(error.to_string()))?;
    for item in entries {
        let mut item = item.map_err(|error| ArchiveError::Codec(error.to_string()))?;
        let raw = String::from_utf8_lossy(&item.path_bytes()).into_owned();
        if !keep(&raw) {
            continue;
        }
        let size = item.size();
        let header = item.header().clone();
        builder
            .append(&header, &mut item)
            .map_err(|error| ArchiveError::Codec(error.to_string()))?;
        done += size;
        progress(done, total);
        copied += 1;
    }
    Ok(copied)
}

fn rebuild_zip(
    archive: &Path,
    temp: &Path,
    keep: &dyn Fn(&str) -> bool,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<usize, ArchiveError> {
    let file = File::open(archive).map_err(|error| ArchiveError::io(archive, error))?;
    let mut reader = zip::ZipArchive::new(file)?;
    let out = File::create(temp).map_err(|error| ArchiveError::io(temp, error))?;
    let mut writer = zip::ZipWriter::new(out);
    let mut copied = 0_usize;
    let mut done = 0_u64;
    for index in 0..reader.len() {
        let mut item = reader.by_index(index)?;
        let name = item.name().to_string();
        if !keep(&name) {
            continue;
        }
        let options =
            SimpleFileOptions::default().unix_permissions(item.unix_mode().unwrap_or(0o644));
        if item.is_dir() {
            writer.add_directory(&name, options)?;
        } else {
            writer.start_file(&name, options)?;
            done += io::copy(&mut item, &mut writer).map_err(|error| ArchiveError::io(temp, error))?;
            progress(done, total);
        }
        copied += 1;
    }
    writer.finish()?;
    Ok(copied)
}

fn open_reader(path: &Path, format: Format) -> Result<Box<dyn Read>, ArchiveError> {
    let file = File::open(path).map_err(|error| ArchiveError::io(path, error))?;
    Ok(match format {
        Format::TarGz => Box::new(GzDecoder::new(file)),
        _ => Box::new(file),
    })
}

fn apply_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

fn system_time_from_zip(when: zip::DateTime) -> SystemTime {
    let days = days_from_civil(when.year() as i64, when.month() as u32, when.day() as u32);
    let seconds = days * 86_400
        + when.hour() as i64 * 3600
        + when.minute() as i64 * 60
        + when.second() as i64;
    UNIX_EPOCH
        .checked_add(Duration::from_secs(seconds.max(0) as u64))
        .unwrap_or(UNIX_EPOCH)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u64;
    let mp = if month > 2 { month - 3 } else { month + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

pub fn tree_size(path: &Path) -> Result<u64, io::Error> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        let mut total = 0;
        for entry in fs::read_dir(path)? {
            total += tree_size(&entry?.path()).unwrap_or(0);
        }
        Ok(total)
    } else {
        Ok(meta.len())
    }
}

/// Human-readable size, base 1024.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
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

/// `YYYY-MM-DD HH:MM` (UTC) for the entry table.
pub fn format_time(time: Option<SystemTime>) -> String {
    let Some(time) = time else {
        return "—".to_string();
    };
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn temp_dir(tag: &str) -> PathBuf {
        static TEST_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aether-archive-{tag}-{}-{stamp}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture() -> PathBuf {
        let root = temp_dir("fixture");
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/readme.md"), "# archive\n").unwrap();
        fs::write(root.join("src/nested/data.txt"), "0123456789").unwrap();
        fs::write(root.join("src/nested/run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        root
    }

    #[test]
    fn detects_formats_by_extension_and_magic() {
        let root = fixture();
        let tar_gz = root.join("bundle.tar.gz");
        create_tar_gz(&tar_gz, &[root.join("src")], &mut |_, _| {}).unwrap();
        assert_eq!(detect_format(&tar_gz).unwrap(), Format::TarGz);

        let disguised = root.join("data.bin");
        fs::copy(&tar_gz, &disguised).unwrap();
        assert_eq!(
            detect_format(&disguised).unwrap(),
            Format::TarGz,
            "magic bytes win over the extension"
        );

        let tar = root.join("bundle.tar");
        let file = File::create(&tar).unwrap();
        let mut builder = tar::Builder::new(file);
        builder
            .append_path_with_name(root.join("src/readme.md"), "readme.md")
            .unwrap();
        builder.into_inner().unwrap();
        assert_eq!(detect_format(&tar).unwrap(), Format::Tar);

        let zip_path = root.join("bundle.zip");
        write_test_zip(&zip_path, &[("a.txt", b"aaa"), ("dir/b.txt", b"bbbb")]);
        assert_eq!(detect_format(&zip_path).unwrap(), Format::Zip);

        let unknown = root.join("notes.txt");
        fs::write(&unknown, "hello").unwrap();
        assert!(matches!(
            detect_format(&unknown),
            Err(ArchiveError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn lists_tar_gz_and_zip_members() {
        let root = fixture();
        let tar_gz = root.join("bundle.tar.gz");
        create_tar_gz(&tar_gz, &[root.join("src")], &mut |_, _| {}).unwrap();
        let (format, entries) = list(&tar_gz).unwrap();
        assert_eq!(format, Format::TarGz);
        assert!(entries.iter().any(|entry| entry.path == "src/readme.md"));
        let data = entries
            .iter()
            .find(|entry| entry.path == "src/nested/data.txt")
            .expect("nested file listed");
        assert_eq!(data.size, 10);
        assert!(!data.is_dir);
        assert!(data.modified.is_some());
        assert!(data.mode & 0o777 != 0);

        let zip_path = root.join("bundle.zip");
        write_test_zip(&zip_path, &[("a.txt", b"aaa"), ("dir/b.txt", b"bbbb")]);
        let (format, entries) = list(&zip_path).unwrap();
        assert_eq!(format, Format::Zip);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].size, 3);
    }

    #[test]
    fn extracts_tar_gz_with_progress_and_permissions() {
        let root = fixture();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                root.join("src/nested/run.sh"),
                fs::Permissions::from_mode(0o750),
            )
            .unwrap();
        }
        let tar_gz = root.join("bundle.tar.gz");
        create_tar_gz(&tar_gz, &[root.join("src")], &mut |_, _| {}).unwrap();

        let dest = root.join("out");
        let mut calls = 0;
        let extracted = extract_all(&tar_gz, &dest, &mut |done, total| {
            calls += 1;
            assert!(done <= total);
        })
        .unwrap();
        assert!(extracted >= 3);
        assert!(calls >= 2, "progress is reported per file");
        assert_eq!(
            fs::read_to_string(dest.join("src/readme.md")).unwrap(),
            "# archive\n"
        );
        assert_eq!(
            fs::read_to_string(dest.join("src/nested/data.txt")).unwrap(),
            "0123456789"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dest.join("src/nested/run.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o750, "tar permissions survive extraction");
        }
    }

    #[test]
    fn extracts_only_selected_zip_members() {
        let root = fixture();
        let zip_path = root.join("bundle.zip");
        write_test_zip(&zip_path, &[("keep.txt", b"keep"), ("skip.txt", b"skip")]);
        let dest = root.join("out");
        let extracted = extract_selected(
            &zip_path,
            &dest,
            &["keep.txt".to_string()],
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(extracted, 1);
        assert_eq!(fs::read_to_string(dest.join("keep.txt")).unwrap(), "keep");
        assert!(!dest.join("skip.txt").exists());

        let all = extract_all(&zip_path, &root.join("all"), &mut |_, _| {}).unwrap();
        assert_eq!(all, 2);
        assert!(root.join("all/skip.txt").exists());
    }

    #[test]
    fn creates_tar_gz_from_files_and_reopens_it() {
        let root = fixture();
        let archive = root.join("created.tar.gz");
        let sources = vec![root.join("src/readme.md"), root.join("src/nested")];
        let mut progress_calls = 0;
        let written = create_tar_gz(&archive, &sources, &mut |_, _| progress_calls += 1).unwrap();
        assert!(written > 0);
        assert_eq!(progress_calls, 2);

        let (format, entries) = list(&archive).unwrap();
        assert_eq!(format, Format::TarGz);
        let names: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert!(names.contains(&"readme.md"));
        assert!(names.contains(&"nested/data.txt"));

        let dest = root.join("round-trip");
        extract_all(&archive, &dest, &mut |_, _| {}).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("nested/data.txt")).unwrap(),
            "0123456789"
        );
    }

    #[test]
    fn deletes_entries_by_rebuilding_tar_and_zip() {
        let root = fixture();
        let tar_gz = root.join("bundle.tar.gz");
        create_tar_gz(&tar_gz, &[root.join("src")], &mut |_, _| {}).unwrap();
        let before = list(&tar_gz).unwrap().1.len();
        let remaining = delete_entries(
            &tar_gz,
            &["src/nested/run.sh".to_string()],
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(remaining, before - 1);
        let (_, entries) = list(&tar_gz).unwrap();
        assert!(!entries.iter().any(|entry| entry.path == "src/nested/run.sh"));
        let dest = root.join("tar-out");
        extract_all(&tar_gz, &dest, &mut |_, _| {}).unwrap();
        assert!(!dest.join("src/nested/run.sh").exists());

        let zip_path = root.join("bundle.zip");
        write_test_zip(
            &zip_path,
            &[("a.txt", b"aaa"), ("b.txt", b"bbb"), ("c.txt", b"ccc")],
        );
        let remaining = delete_entries(&zip_path, &["b.txt".to_string()], &mut |_, _| {}).unwrap();
        assert_eq!(remaining, 2);
        let (_, entries) = list(&zip_path).unwrap();
        let names: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(names, vec!["a.txt", "c.txt"]);
        let dest = root.join("zip-out");
        extract_all(&zip_path, &dest, &mut |_, _| {}).unwrap();
        assert_eq!(fs::read_to_string(dest.join("c.txt")).unwrap(), "ccc");
    }

    #[test]
    fn rejects_malicious_tar_member() {
        let root = fixture();
        let evil = root.join("evil.tar");
        write_raw_tar_member(&evil, "../evil.txt", b"pwned");
        let dest = root.join("evil-out");
        let result = extract_all(&evil, &dest, &mut |_, _| {});
        match result {
            Err(ArchiveError::MaliciousEntry(name)) => assert_eq!(name, "../evil.txt"),
            other => panic!("expected a traversal refusal, got {other:?}"),
        }
        assert!(!root.join("evil.txt").exists(), "nothing escapes the destination");
        assert!(!dest.join("evil.txt").exists());
    }

    #[test]
    fn rejects_malicious_zip_member() {
        let root = fixture();
        let evil = root.join("evil.zip");
        let file = File::create(&evil).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("../evil.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"pwned").unwrap();
        writer.finish().unwrap();

        let dest = root.join("evil-zip-out");
        let result = extract_all(&evil, &dest, &mut |_, _| {});
        assert!(
            matches!(result, Err(ArchiveError::MaliciousEntry(_))),
            "zip-slip refused: {result:?}"
        );
        assert!(!root.join("evil.txt").exists());
    }

    #[test]
    fn guard_rejects_absolute_and_parent_paths() {
        assert!(matches!(
            safe_entry_path("/etc/passwd"),
            Err(ArchiveError::MaliciousEntry(_))
        ));
        assert!(matches!(
            safe_entry_path("a/../../b"),
            Err(ArchiveError::MaliciousEntry(_))
        ));
        assert!(matches!(
            safe_entry_path(".."),
            Err(ArchiveError::MaliciousEntry(_))
        ));
        assert_eq!(
            safe_entry_path("./docs/readme.md").unwrap(),
            PathBuf::from("docs/readme.md")
        );
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_time(None), "—");
    }

    #[test]
    fn creating_without_sources_is_refused_and_tree_size_measures_files() {
        let root = fixture();
        let archive = root.join("empty.tar.gz");
        match create_tar_gz(&archive, &[], &mut |_, _| {}) {
            Err(ArchiveError::InvalidName(name)) => assert_eq!(name, "nothing to archive"),
            other => panic!("expected an InvalidName error, got {other:?}"),
        }
        assert!(!archive.exists(), "no archive is created for an empty request");

        assert_eq!(tree_size(&root.join("src/readme.md")).unwrap(), 10);
        let data = fs::metadata(root.join("src/nested/data.txt")).unwrap().len();
        let script = fs::metadata(root.join("src/nested/run.sh")).unwrap().len();
        let nested = tree_size(&root.join("src/nested")).unwrap();
        assert_eq!(nested, data + script);
        assert!(tree_size(&root.join("src")).unwrap() >= nested);
        assert!(tree_size(&root.join("missing")).is_err());
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        let _ = fs::remove_dir_all(root);
    }

    // --- fixture builders -------------------------------------------------

    fn write_test_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, data) in files {
            writer
                .start_file(*name, SimpleFileOptions::default().unix_permissions(0o640))
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    /// Hand-build a tar with a raw `..` member name: the tar crate refuses to
    /// write one, and the extractor must refuse to read it.
    fn write_raw_tar_member(path: &Path, raw_name: &str, data: &[u8]) {
        let mut header = [0_u8; 512];
        let name = raw_name.as_bytes();
        assert!(name.len() < 100);
        header[..name.len()].copy_from_slice(name);
        let octal = |header: &mut [u8; 512], offset: usize, len: usize, value: u64| {
            let text = format!("{:0width$o}", value, width = len - 1);
            header[offset..offset + len - 1].copy_from_slice(text.as_bytes());
            header[offset + len - 1] = 0;
        };
        octal(&mut header, 100, 8, 0o644);
        octal(&mut header, 108, 8, 0);
        octal(&mut header, 116, 8, 0);
        octal(&mut header, 124, 12, data.len() as u64);
        octal(&mut header, 136, 12, 0);
        header[148..156].copy_from_slice(&[b' '; 8]);
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        header[263..265].copy_from_slice(b"00");
        let sum: u64 = header.iter().map(|byte| *byte as u64).sum();
        let checksum = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(data);
        let padding = (512 - data.len() % 512) % 512;
        bytes.extend(std::iter::repeat(0_u8).take(padding));
        bytes.extend(std::iter::repeat(0_u8).take(1024));
        fs::write(path, bytes).unwrap();
    }

    #[allow(dead_code)]
    fn tar_from_cursor(bytes: Vec<u8>) -> tar::Archive<Cursor<Vec<u8>>> {
        tar::Archive::new(Cursor::new(bytes))
    }
    #[cfg(unix)]
    #[test]
    fn extraction_refuses_a_symlink_already_in_the_destination() {
        // Regression for the escape found in the P4 review: a symlink that
        // already exists in the destination must not be traversed.
        let root = fixture();
        let outside = temp_dir("outside");
        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("link")).unwrap();

        let tar = root.join("escape.tar");
        write_raw_tar_member(&tar, "link/pwn.txt", b"escaped");

        let error = extract_all(&tar, &dest, &mut |_, _| {}).unwrap_err();
        assert!(
            matches!(error, ArchiveError::MaliciousEntry(_)),
            "tar member through a destination symlink must be refused, got {error:?}"
        );
        assert!(
            !outside.join("pwn.txt").exists(),
            "nothing may be written outside the destination"
        );

        let zip_path = root.join("escape.zip");
        write_test_zip(&zip_path, &[("link/pwn.zip", b"escaped")]);
        let error = extract_all(&zip_path, &dest, &mut |_, _| {}).unwrap_err();
        assert!(matches!(error, ArchiveError::MaliciousEntry(_)));
        assert!(!outside.join("pwn.zip").exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn extraction_refuses_a_symlinked_file_target() {
        // The final path component must not be an existing symlink either:
        // File::create would follow it and overwrite the linked file.
        let root = fixture();
        let outside = temp_dir("outside-file");
        let outside_file = outside.join("secret.txt");
        fs::write(&outside_file, "original").unwrap();
        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink(&outside_file, dest.join("evil.txt")).unwrap();

        let tar = root.join("evil.tar");
        write_raw_tar_member(&tar, "evil.txt", b"overwritten");

        let error = extract_all(&tar, &dest, &mut |_, _| {}).unwrap_err();
        assert!(matches!(error, ArchiveError::MaliciousEntry(_)));
        assert_eq!(
            fs::read_to_string(&outside_file).unwrap(),
            "original",
            "the linked file outside the destination is untouched"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn selecting_a_folder_extracts_its_children() {
        let root = fixture();
        let zip_path = root.join("tree.zip");
        write_test_zip(
            &zip_path,
            &[("dir/one.txt", b"1"), ("dir/sub/two.txt", b"2"), ("other.txt", b"3")],
        );
        let dest = root.join("dest");
        let wanted = vec!["dir".to_string()];
        let extracted = extract_selected(&zip_path, &dest, &wanted, &mut |_, _| {}).unwrap();
        assert_eq!(extracted, 2, "both files under the selected folder");
        assert!(dest.join("dir/one.txt").exists());
        assert!(dest.join("dir/sub/two.txt").exists());
        assert!(!dest.join("other.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selecting_a_folder_for_deletion_removes_its_children() {
        let root = fixture();
        let zip_path = root.join("tree.zip");
        write_test_zip(
            &zip_path,
            &[("dir/one.txt", b"1"), ("dir/two.txt", b"2"), ("keep.txt", b"3")],
        );
        let removed = delete_entries(&zip_path, &["dir".to_string()], &mut |_, _| {}).unwrap();
        assert!(removed >= 1);
        let (_, entries) = list(&zip_path).unwrap();
        let names: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert!(!names.iter().any(|name| name.starts_with("dir")), "got {names:?}");
        assert!(names.contains(&"keep.txt"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nested_directories_are_created_inside_the_destination() {
        // The symlink guard must not break ordinary nested extraction.
        let root = fixture();
        let dest = root.join("dest");
        let zip_path = root.join("nested.zip");
        write_test_zip(&zip_path, &[("a/b/c.txt", b"deep"), ("a/top.txt", b"top")]);
        assert_eq!(extract_all(&zip_path, &dest, &mut |_, _| {}).unwrap(), 2);
        assert_eq!(fs::read_to_string(dest.join("a/b/c.txt")).unwrap(), "deep");
        assert_eq!(fs::read_to_string(dest.join("a/top.txt")).unwrap(), "top");
        let _ = fs::remove_dir_all(&root);
    }

}
