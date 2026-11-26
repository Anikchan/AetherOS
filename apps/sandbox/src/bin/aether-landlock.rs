//! `aether-landlock` — apply a Landlock ruleset, then exec the app (P6-T04).
//!
//! Usage:
//!   aether-landlock --probe [--json]
//!   aether-landlock [--ro PATH]... [--rw PATH]... -- PROGRAM [ARGS]...
//!
//! Fail-closed: when Landlock is unavailable or a rule is rejected, the shim
//! prints one line and exits 126 **without running the program**. It never
//! executes the app unsandboxed.

use std::ffi::CString;

use aether_sandbox::landlock::{self, SHIM_REFUSED_EXIT};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--probe") {
        let status = landlock::probe();
        if args.iter().any(|arg| arg == "--json") {
            println!("{}", serde_json::to_string_pretty(&status).unwrap_or_default());
        } else {
            match &status {
                landlock::LandlockStatus::Available { abi } => println!("LANDLOCK_ABI {abi}"),
                landlock::LandlockStatus::Unavailable { reason } => {
                    println!("LANDLOCK_UNAVAILABLE {reason}");
                    std::process::exit(SHIM_REFUSED_EXIT);
                }
            }
        }
        return;
    }

    if args.first().map(String::as_str) == Some("-h")
        || args.first().map(String::as_str) == Some("--help")
    {
        println!("usage: aether-landlock [--ro PATH]... [--rw PATH]... -- PROGRAM [ARGS]...");
        println!("       aether-landlock --probe [--json]");
        return;
    }

    let invocation = landlock::parse_shim_args(&args).unwrap_or_else(|error| {
        eprintln!("aether-landlock: {error}");
        std::process::exit(2);
    });

    match landlock::apply(&invocation.policy) {
        Ok(applied) => {
            eprintln!(
                "aether-landlock: ABI {} enforced, {} rules, handled {:#x}",
                applied.abi, applied.rules, applied.handled
            );
        }
        Err(error) => {
            // Fail-closed: the program is never exec'd.
            eprintln!("aether-landlock: refusing to start the app: {error}");
            std::process::exit(SHIM_REFUSED_EXIT);
        }
    }

    exec(&invocation)
}

#[cfg(unix)]
fn exec(invocation: &landlock::ShimInvocation) -> ! {
    use std::os::unix::ffi::OsStrExt;

    let program = match CString::new(invocation.program.as_os_str().as_bytes()) {
        Ok(program) => program,
        Err(_) => {
            eprintln!("aether-landlock: program path contains NUL");
            std::process::exit(2);
        }
    };
    let mut argv: Vec<CString> = Vec::with_capacity(invocation.args.len() + 1);
    argv.push(program.clone());
    for arg in &invocation.args {
        match CString::new(arg.as_bytes()) {
            Ok(value) => argv.push(value),
            Err(_) => {
                eprintln!("aether-landlock: argument contains NUL");
                std::process::exit(2);
            }
        }
    }
    let mut pointers: Vec<*const libc::c_char> = argv.iter().map(|arg| arg.as_ptr()).collect();
    pointers.push(std::ptr::null());
    unsafe {
        libc::execvp(program.as_ptr(), pointers.as_ptr());
    }
    eprintln!(
        "aether-landlock: cannot exec {}: {}",
        invocation.program.display(),
        std::io::Error::last_os_error()
    );
    std::process::exit(127);
}

#[cfg(not(unix))]
fn exec(invocation: &landlock::ShimInvocation) -> ! {
    eprintln!(
        "aether-landlock: cannot exec {} on this host",
        invocation.program.display()
    );
    std::process::exit(127);
}
