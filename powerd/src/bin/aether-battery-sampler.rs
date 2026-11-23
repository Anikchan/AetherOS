//! `aether-battery-sampler` — battery benchmark sampler (P5-T14, 04 §11.2).
//!
//! Samples battery percentage and power at a fixed interval and writes CSV
//! for W1 (idle), W2 (browsing) and W3 (video) runs. It never fabricates
//! readings: without a battery it exits non-zero and says so.
//!
//! Usage:
//!   aether-battery-sampler [--hours H] [--interval SECS] [--out FILE]
//!                          [--label W1|W2|W3] [--dry-run-samples N]

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aether_powerd::telemetry::BatteryReader;
use aether_powerd::types::{BatteryTelemetry, BatteryStatus};

fn main() {
    let mut hours = 1.0f64;
    let mut interval = Duration::from_secs(300);
    let mut out: Option<PathBuf> = None;
    let mut label = "W1".to_string();
    let mut dry_run_samples = 0usize;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--hours" => hours = args.next().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            "--interval" => {
                interval = Duration::from_secs(
                    args.next().and_then(|v| v.parse().ok()).unwrap_or(300),
                )
            }
            "--out" => out = args.next().map(PathBuf::from),
            "--label" => label = args.next().unwrap_or_else(|| "W1".into()),
            "--dry-run-samples" => {
                dry_run_samples = args.next().and_then(|v| v.parse().ok()).unwrap_or(0)
            }
            "-h" | "--help" => {
                println!("usage: aether-battery-sampler [--hours H] [--interval SECS] [--out FILE] [--label W1|W2|W3] [--dry-run-samples N]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let mut writer: Box<dyn Write> = match &out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::File::create(path) {
                Ok(file) => Box::new(std::io::BufWriter::new(file)),
                Err(e) => {
                    eprintln!("cannot write {}: {e}", path.display());
                    std::process::exit(1);
                }
            }
        }
        None => Box::new(std::io::stdout()),
    };

    let _ = writeln!(
        writer,
        "timestamp,label,percent,source,charging,rate_w,energy_wh,health_percent,cycle_count"
    );

    if dry_run_samples > 0 {
        // Harness self-test: synthetic rows, clearly labelled `dry-run`.
        for index in 0..dry_run_samples {
            let percent = 100.0 - index as f32 * 2.0;
            let _ = writeln!(
                writer,
                "{},dry-run,{:.1},battery,false,10.0,40.0,90.0,100",
                index as u64 * interval.as_secs(),
                percent
            );
        }
        let _ = writer.flush();
        println!("P5T14_SAMPLER_DRYRUN rows={dry_run_samples} label={label}");
        return;
    }

    let reader = BatteryReader::system();
    let mut last: Option<BatteryStatus> = None;
    let deadline = Instant::now() + Duration::from_secs_f64(hours * 3600.0);
    let mut rows = 0u64;
    loop {
        match reader.read() {
            Ok(status) => {
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(
                    writer,
                    "{epoch},{label},{:.1},{:?},{},{},{:?},{:?},{:?}",
                    status.percent,
                    status.source,
                    status.charging,
                    status.energy_rate_w.map(|v| format!("{v:.2}")).unwrap_or_default(),
                    status.energy_now_wh.map(|v| format!("{v:.2}")).unwrap_or_default(),
                    status.health_percent.map(|v| format!("{v:.1}")).unwrap_or_default(),
                    status.cycle_count.map(|v| v.to_string()).unwrap_or_default(),
                );
                rows += 1;
                last = Some(status);
            }
            Err(error) => {
                eprintln!("no battery telemetry: {error}");
                eprintln!("Battery benchmark results from a host without a battery do not count (04 §10).");
                std::process::exit(1);
            }
        }
        let _ = writer.flush();
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
    }

    println!(
        "P5T14_SAMPLER_DONE label={label} rows={rows} final_percent={:.1}",
        last.as_ref().map(|s| s.percent).unwrap_or(0.0)
    );
}
