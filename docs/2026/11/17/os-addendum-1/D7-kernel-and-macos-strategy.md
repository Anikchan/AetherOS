# D7 — Kernel Strategy and macOS App Requirements

This document answers: "I don't want to rule out a separate kernel completely, but for now we pick the closest kernel. To run macOS apps, what would we need?"

## 1. Decision (Now)

- **Use the Linux kernel** (ADR-001 remains Proposed until the owner accepts it in P0-T02).
- **Keep a separate kernel possible** by isolating kernel-specific code (section 3).
- **Do not build a kernel now.** A new kernel would have to rebuild driver support from scratch. That is the requirement that most often stops new operating systems, and it conflicts with Q2 ("any system").

## 2. Kernel Options Compared

Rating for our needs: Strong, Partial, Weak. Based on general knowledge of each project; to be confirmed in P0 if the owner wants to change the choice.

| Requirement | Linux (chosen) | FreeBSD | Redox OS (Rust microkernel) | seL4 (verified microkernel) | Own kernel |
| --- | --- | --- | --- | --- | --- |
| Driver coverage for laptops and PCs (Q2) | Strong | Partial | Weak | Weak | Weak (years to build) |
| Process control for suspend and memory limits (F2) | Strong (cgroup v2, freezer, PSI) | Partial (jails and rctl) | Partial (design-specific) | Partial (needs own services) | Must be built |
| Compressed memory and reclaim (F2, D3) | Strong (zram, memory.reclaim) | Partial | Weak | Weak | Must be built |
| Power management on laptops (F7) | Strong (cpufreq, runtime PM, UPower) | Partial | Weak | Weak | Must be built |
| Running Windows programs (F9, Wine) | Strong (Wine is mature on Linux) | Partial | Weak | Weak | Weak |
| GPU and graphics stack (F1, D1) | Strong (Mesa, DRM) | Partial | Weak | Weak | Weak |
| Fast boot (F11) | Strong with tuning | Strong | Partial | Strong | Depends |
| Security model | Strong with sandbox tools (namespaces, Landlock, seccomp) | Strong (capsicum, jails) | Strong (design) | Very strong (proofs) | Depends |
| License | GPL-2.0 (kernel); compatible with our design | BSD | MIT | GPL-2.0 with commercial option (verify) | Ours |
| Work for one person | Low (we configure and integrate) | Medium | Very high | Very high | Extreme |

Conclusion: Linux is the only option that meets the hardware, power, and process-control requirements within this plan. Redox OS and seL4 are useful reference projects for a long-term design (for example, to study how a Rust-based system is organized). They are not realistic for the current scope.

## 3. Keeping a Separate Kernel Possible

Rule: only one crate may use Linux-specific interfaces. Everything else uses that crate's interface.

| Crate | Responsibility | Linux interfaces used |
| --- | --- | --- |
| `nexa-kernel` (interface) | Process groups, freeze, reclaim, pressure, power sources, device power settings, sandbox setup | cgroup v2, PSI, zram, namespaces, Landlock, seccomp, sysfs |
| Services (`nexa-lifecycled`, `nexa-powerd`, `nexa-pkg`, `nexa-sandbox`) | Policy only | Through `nexa-kernel` only |
| Shell and apps | No kernel calls | Through services |

Interface sketch (contract only):

```rust
pub trait ProcessGroups {
    fn create_group(&self, name: &str) -> std::io::Result<GroupId>;
    fn add_process(&self, group: GroupId, pid: u32) -> std::io::Result<()>;
    fn freeze(&self, group: GroupId, frozen: bool) -> std::io::Result<()>;
    fn reclaim(&self, group: GroupId, bytes: u64) -> std::io::Result<u64>;
    fn pressure(&self) -> std::io::Result<PressureSnapshot>;
}

pub trait PowerSources {
    fn battery(&self) -> std::io::Result<BatterySnapshot>;
    fn set_device_power_saving(&self, device: &str, on: bool) -> std::io::Result<()>;
}

pub trait Sandbox {
    fn restrict(&self, profile: &SandboxProfile) -> std::io::Result<()>;
}
```

Cost: roughly 40 hours in P1 and P3, included in the plan. Benefit: if the owner ever wants a different kernel, the change is limited to one crate and its tests. The rest of the system does not change.

## 4. What Running macOS Apps Would Require

A macOS app is a Mach-O program. It expects Apple's operating system services, which are not open source in full. The list below shows what a compatibility layer must provide.

| # | Component | What it does | Open-source? | Difficulty |
| --- | --- | --- | --- | --- |
| 1 | Mach-O loader | Loads macOS program files (Intel and universal binaries) | Yes (reference projects exist) | High |
| 2 | Dynamic linker and system library (dyld, libSystem equivalent) | Loads libraries and provides C library entry points | Partly | High |
| 3 | XNU-compatible kernel services | Mach traps, Mach ports and messages, BSD system calls with macOS numbering | Partly (XNU is open source under APSL; parts are GPL-compatible only through a module) | Very high |
| 4 | Service manager (launchd equivalent) | Starts and manages macOS system services | Yes (concept) | Medium |
| 5 | Objective-C and Swift runtimes | Run Objective-C and Swift code | Yes (open-source runtimes exist) | Medium |
| 6 | Core libraries (Foundation, Core Foundation) | Strings, data, files, collections | Partly (open-source versions exist) | High |
| 7 | Graphics and UI frameworks (AppKit, Core Graphics, Core Animation, Metal, Core Text, WebKit) | Windows, drawing, text, GPU | Mostly no (proprietary); must be rewritten clean-room | Extreme |
| 8 | Window and input bridge to our compositor | Connects macOS windows to our desktop | Ours | High |
| 9 | File layout (`/Applications`, `~/Library`, preferences) | Expected folder structure | Ours | Low |
| 10 | Code signing and security checks | Apps expect signing checks; we would need to allow or emulate | Ours | Medium |

Reference project: Darling is an open-source compatibility layer that runs some macOS programs on Linux. In a search on 2026-11-17, it was described as partly working (status to be re-checked before any decision). It uses a modified XNU kernel packaged as a Linux kernel module. It is a research reference, not a product we can use directly.

## 5. Legal Limits

- Apple's graphics, UI, and system frameworks are proprietary. We cannot copy them. A clean-room rewrite is required.
- macOS itself is licensed for Apple hardware. We do not ship macOS, and we do not use macOS system files.
- XNU source code is under the APSL license, which has conditions that must be checked before any reuse.
- Any use of macOS apps is limited to the apps' own terms. Some apps need Apple-specific services (iCloud, Apple ID) that cannot be provided.

## 6. Verdict

| Option | Feasible for us? | Effort | Recommendation |
| --- | --- | --- | --- |
| Run macOS GUI apps | No, within this plan | Years of team-scale work (components 3, 6, 7 are the largest) | Do not plan it |
| Run macOS command-line tools only | Partly, as a research project | Large, but smaller than GUI (needs components 1, 2, 3, 5, 6) | Research track after M5, only if the owner wants it |
| Use cross-platform equivalents (Electron, Java, Flutter, web, or Windows/Linux builds of the same app) | Yes | Low | Recommended path (F9 section 5) |
| Run macOS in a virtual machine | Only on Apple hardware, and only within Apple's license | Medium | Out of scope (Q5) |

Owner decision (2026-11-17): accept "no macOS apps" for Personal Edition, with the research track left open. The OS must still run the owner's own apps (such as PowerGuard and AI monitoring) as native or cross-platform bundles.

## 7. Reading List (for the research track)

- Darling project documentation and source (open-source macOS compatibility layer for Linux).
- Redox OS documentation (Rust operating system; reference for system design).
- seL4 documentation (verified microkernel; reference for security design).
- Apple's open-source releases (XNU, Objective-C runtime, Swift core libraries), each with its own license; check terms before reuse.
