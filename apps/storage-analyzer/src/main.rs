//! `aether-storage-analyzer` — disk usage map and large file finder (Pack G).
//!
//! Usage: aether-storage-analyzer [PATH] [--json] [--top N] [--min-size BYTES]

use std::path::PathBuf;

use aether_storage_analyzer::{large_files, render_map, scan};

fn main() {
    let mut path = PathBuf::from(".");
    let mut json = false;
    let mut top = 20usize;
    let mut min_size = 100 * 1024 * 1024u64;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--top" => top = args.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            "--min-size" => {
                min_size = args.next().and_then(|v| v.parse().ok()).unwrap_or(min_size)
            }
            "-h" | "--help" => {
                println!("usage: aether-storage-analyzer [PATH] [--json] [--top N] [--min-size BYTES]");
                return;
            }
            value => path = PathBuf::from(value),
        }
    }

    match scan(&path, top) {
        Ok(report) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).unwrap_or_default()
                );
            } else {
                print!("{}", render_map(&report, 10));
                let candidates = large_files(&report, min_size);
                if !candidates.is_empty() {
                    println!("\nCleanup candidates over {}:", bytes_label(min_size));
                    for file in candidates {
                        println!("  {:>10}  {}", bytes_label(file.bytes), file.path.display());
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("scan failed: {error}");
            std::process::exit(1);
        }
    }
}

fn bytes_label(bytes: u64) -> String {
    aether_storage_analyzer::human_size(bytes)
}
