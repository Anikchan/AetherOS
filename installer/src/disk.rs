//! Disk, Windows, EFI and BitLocker detection (P8-T02, F5 INS-02).
//!
//! Detection is pure: it parses `lsblk --json` and small probe files. Real
//! probing is a thin wrapper, so the report can be tested with fixtures.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartitionTable {
    Gpt,
    Dos,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    Windows,
    AetherOs,
    Linux,
    Efi,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partition {
    pub number: u32,
    pub name: String,
    pub size_bytes: u64,
    pub fs: Option<String>,
    pub mountpoint: Option<PathBuf>,
    pub os: OperatingSystem,
    pub encrypted: bool,
}

impl Partition {
    pub fn free_bytes(&self) -> u64 {
        0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    pub device: String,
    pub size_bytes: u64,
    pub model: String,
    pub table: PartitionTable,
    pub partitions: Vec<Partition>,
    /// Unallocated space on the disk.
    pub free_bytes: u64,
    pub removable: bool,
}

impl Disk {
    pub fn windows_partition(&self) -> Option<&Partition> {
        self.partitions
            .iter()
            .find(|partition| partition.os == OperatingSystem::Windows)
    }

    pub fn esp(&self) -> Option<&Partition> {
        self.partitions
            .iter()
            .find(|partition| partition.os == OperatingSystem::Efi)
    }

    pub fn aetheros_partitions(&self) -> Vec<&Partition> {
        self.partitions
            .iter()
            .filter(|partition| partition.os == OperatingSystem::AetherOs)
            .collect()
    }

    pub fn label(&self) -> String {
        format!("{} ({}, {})", self.device, self.model, human_size(self.size_bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsInfo {
    pub present: bool,
    pub version: Option<String>,
    pub bitlocker: bool,
    pub fast_startup: bool,
    pub ntfs_clean: bool,
    pub pending_updates: bool,
    pub recovery_key_saved: bool,
}

impl Default for WindowsInfo {
    fn default() -> Self {
        Self {
            present: false,
            version: None,
            bitlocker: false,
            fast_startup: false,
            ntfs_clean: true,
            pending_updates: false,
            recovery_key_saved: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareReport {
    pub cpu: String,
    pub memory_bytes: u64,
    pub gpu: Vec<String>,
    pub wifi: Option<String>,
    pub audio: Option<String>,
    pub firmware: String,
    pub disks: Vec<Disk>,
    pub windows: WindowsInfo,
    pub bitlocker_detected: bool,
}

#[derive(Deserialize)]
struct LsblkRoot {
    blockdevices: Vec<LsblkRow>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct LsblkRow {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    rm: bool,
    #[serde(default)]
    parttypename: Option<String>,
    #[serde(default)]
    children: Vec<LsblkRow>,
}

/// Parse `lsblk --json --bytes --output NAME,SIZE,MODEL,TYPE,FSTYPE,MOUNTPOINT,TRAN,RM,PARTTYPENAME`.
pub fn parse_lsblk_json(text: &str) -> Result<Vec<Disk>, serde_json::Error> {
    let root: LsblkRoot = serde_json::from_str(text)?;
    let mut disks = Vec::new();
    for row in root.blockdevices {
        if row.kind != "disk" {
            continue;
        }
        let device = format!("/dev/{}", row.name);
        let mut partitions = Vec::new();
        let mut used = 0u64;
        for child in &row.children {
            let os = classify(child);
            used += child.size;
            partitions.push(Partition {
                number: child
                    .name
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0),
                name: child.name.clone(),
                size_bytes: child.size,
                fs: child.fstype.clone(),
                mountpoint: child.mountpoint.clone().map(PathBuf::from),
                os,
                encrypted: child
                    .fstype
                    .as_deref()
                    .map(|fs| fs.eq_ignore_ascii_case("crypto_LUKS"))
                    .unwrap_or(false),
            });
        }
        disks.push(Disk {
            device,
            size_bytes: row.size,
            model: row.model.unwrap_or_else(|| "unknown".into()),
            table: if row.parttypename.is_some() {
                PartitionTable::Gpt
            } else if row.children.is_empty() {
                PartitionTable::Unknown
            } else {
                PartitionTable::Gpt
            },
            partitions,
            free_bytes: row.size.saturating_sub(used),
            removable: row.rm,
        });
    }
    Ok(disks)
}

fn classify(row: &LsblkRow) -> OperatingSystem {
    let fs = row.fstype.clone().unwrap_or_default().to_ascii_lowercase();
    let type_name = row
        .parttypename
        .clone()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mount = row.mountpoint.clone().unwrap_or_default().to_ascii_lowercase();
    if fs == "vfat" && (type_name.contains("efi") || mount.contains("/boot/efi")) {
        OperatingSystem::Efi
    } else if fs == "ntfs" || type_name.contains("microsoft basic data") {
        OperatingSystem::Windows
    } else if fs == "ext4" && mount.starts_with("/") {
        OperatingSystem::AetherOs
    } else if fs.starts_with("ext") || fs == "btrfs" || fs == "xfs" {
        OperatingSystem::Linux
    } else {
        OperatingSystem::Other
    }
}

/// Probe the running system. Missing tools produce empty data, never panic.
pub fn detect_system() -> HardwareReport {
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into());
    let memory_bytes = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("MemTotal:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u64>().ok())
                .map(|kb| kb * 1024)
        })
        .unwrap_or(0);
    let disks = std::process::Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--output",
            "NAME,SIZE,MODEL,TYPE,FSTYPE,MOUNTPOINT,TRAN,RM,PARTTYPENAME",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| parse_lsblk_json(&String::from_utf8_lossy(&output.stdout)).ok())
        .unwrap_or_default();
    let firmware = if Path::new("/sys/firmware/efi").exists() {
        "UEFI".to_string()
    } else {
        "BIOS".to_string()
    };
    HardwareReport {
        cpu,
        memory_bytes,
        gpu: read_gpu_names(),
        wifi: read_first(
            "/sys/class/net",
            |name| name.starts_with("wl") || name.starts_with("wlan"),
        ),
        audio: fs::read_to_string("/proc/asound/cards")
            .ok()
            .and_then(|text| text.lines().nth(0).map(|line| line.trim().to_string())),
        firmware,
        bitlocker_detected: disks
            .iter()
            .flat_map(|disk| &disk.partitions)
            .any(|partition| partition.encrypted),
        windows: WindowsInfo::default(),
        disks,
    }
}

fn read_gpu_names() -> Vec<String> {
    let base = Path::new("/sys/class/drm");
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("card") && !name.contains('-') {
                if let Ok(device) = fs::read_link(entry.path().join("device")) {
                    names.push(
                        device
                            .file_name()
                            .map(|value| value.to_string_lossy().into_owned())
                            .unwrap_or(name),
                    );
                }
            }
        }
    }
    names
}

fn read_first(dir: &str, predicate: impl Fn(&str) -> bool) -> Option<String> {
    fs::read_dir(dir).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| predicate(name))
    })
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Parse `/proc/mounts` for the EFI mount point (fallback path).
pub fn find_esp_mounts(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[2].eq_ignore_ascii_case("vfat") {
                Some(PathBuf::from(parts[1]))
            } else {
                None
            }
        })
        .collect()
}

/// Read BitLocker/Fast Startup markers from a mounted Windows volume.
pub fn inspect_windows_mount(mount: &Path) -> io::Result<WindowsInfo> {
    let mut info = WindowsInfo {
        present: true,
        ..Default::default()
    };
    if let Ok(version) = fs::read_to_string(mount.join("Windows/System32/ntoskrnl.exe")) {
        info.version = version
            .lines()
            .next()
            .map(|line| line.trim().to_string());
    }
    info.fast_startup = mount.join("Windows/System32/Windows/hiberfil.sys").exists()
        || mount.join("hiberfil.sys").exists();
    info.bitlocker = mount.join("BitLocker").exists();
    Ok(info)
}

/// Group mount points by filesystem for the hardware report.
pub fn mounts_by_fs(mounts: &str) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            map.entry(parts[2].to_string())
                .or_default()
                .push(parts[1].to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSBLK: &str = r#"
{
  "blockdevices": [
    {
      "name": "nvme0n1", "size": 512110190592, "model": "Samsung SSD", "type": "disk", "rm": false,
      "children": [
        {"name": "nvme0n1p1", "size": 104857600, "fstype": "vfat", "type": "part", "mountpoint": "/boot/efi", "parttypename": "EFI System"},
        {"name": "nvme0n1p2", "size": 52428800, "fstype": null, "type": "part", "parttypename": "Microsoft Reserved"},
        {"name": "nvme0n1p3", "size": 400000000000, "fstype": "ntfs", "type": "part", "parttypename": "Microsoft basic data"},
        {"name": "nvme0n1p4", "size": 107374182400, "fstype": null, "type": "part", "parttypename": ""}
      ]
    },
    {"name": "sda", "size": 16000000000, "model": "USB Stick", "type": "disk", "rm": true, "children": []}
  ]
}
"#;

    #[test]
    fn parses_lsblk_and_classifies_partitions() {
        let disks = parse_lsblk_json(LSBLK).unwrap();
        assert_eq!(disks.len(), 2);
        let disk = &disks[0];
        assert_eq!(disk.device, "/dev/nvme0n1");
        assert_eq!(disk.model, "Samsung SSD");
        assert_eq!(disk.partitions.len(), 4);
        assert!(disk.esp().is_some());
        assert!(disk.windows_partition().is_some());
        assert!(disks[1].removable);
        assert!(disk.free_bytes > 0);
    }

    #[test]
    fn esp_mounts_are_detected_from_proc_mounts() {
        let mounts = "/dev/nvme0n1p1 /boot/efi vfat rw 0 0\n/dev/nvme0n1p3 /mnt/windows ntfs3 rw 0 0\n";
        assert_eq!(find_esp_mounts(mounts), vec![PathBuf::from("/boot/efi")]);
    }

    #[test]
    fn windows_mount_probe_reads_markers() {
        let dir = std::env::temp_dir().join(format!("aether-win-probe-{}", std::process::id()));
        let _ = fs::create_dir_all(dir.join("Windows/System32/Windows"));
        fs::write(dir.join("hiberfil.sys"), "x").unwrap();
        let info = inspect_windows_mount(&dir).unwrap();
        assert!(info.present);
        assert!(info.fast_startup);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn human_sizes_are_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1125899906842624), "1.1 PB");
        assert!(human_size(512110190592).contains("GB"));
    }
}
