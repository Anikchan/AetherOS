//! P9-T06 (partial) — input fuzzing for disk detection and the installer
//! planner. `lsblk` output is untrusted input from the environment; parsing
//! must never panic.

use aether_installer::disk::parse_lsblk_json;
use aether_installer::plan_install;
use aether_installer::precheck::InstallMode;
use aether_installer::WindowsInfo;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

#[test]
fn lsblk_parser_never_panics_on_random_json() {
    let mut lcg = Lcg(0x1234_5678_9abc_def0);
    const ALPHABET: &[u8] = b"{}[],:\"\\ 0123456789abcdefghijklmnopqrstuvwxyz\n\t";
    for _ in 0..10_000 {
        let len = (lcg.next() % 500) as usize;
        let mut text = String::with_capacity(len);
        for _ in 0..len {
            text.push(ALPHABET[(lcg.next() as usize) % ALPHABET.len()] as char);
        }
        let _ = parse_lsblk_json(&text);
    }
}

#[test]
fn lsblk_parser_handles_structured_mutations() {
    let base = r#"{"blockdevices":[{"name":"sda","size":100,"model":"X","type":"disk","children":[{"name":"sda1","size":50,"fstype":"ntfs","type":"part"}]}]}"#;
    let mut lcg = Lcg(0x0f0f_0f0f_1111_2222);
    for _ in 0..5_000 {
        let mut text = base.to_string();
        let position = (lcg.next() as usize) % text.len();
        text.insert(position, b"{}[]\":,\\"[((lcg.next() as usize) % 8)] as char);
        let _ = parse_lsblk_json(&text);
    }
}

#[test]
fn planner_never_panics_on_extreme_sizes() {
    use aether_installer::disk::{Disk, Partition, PartitionTable};
    let mut lcg = Lcg(0x9999_8888_7777_6666);
    for _ in 0..2_000 {
        let size = lcg.next();
        let free = lcg.next() % (size.max(1));
        let disk = Disk {
            device: "/dev/fuzz".into(),
            size_bytes: size,
            model: "fuzz".into(),
            table: PartitionTable::Gpt,
            partitions: vec![Partition {
                number: 1,
                name: "fuzz1".into(),
                size_bytes: lcg.next(),
                fs: Some("ntfs".into()),
                mountpoint: None,
                os: aether_installer::OperatingSystem::Windows,
                encrypted: lcg.next() % 2 == 0,
            }],
            free_bytes: free,
            removable: lcg.next() % 10 == 0,
        };
        let windows = WindowsInfo::default();
        let _ = plan_install(
            &disk,
            &windows,
            InstallMode::Alongside,
            lcg.next() % 10_000,
            (lcg.next() % 128) as u32,
            None,
        );
    }
}
