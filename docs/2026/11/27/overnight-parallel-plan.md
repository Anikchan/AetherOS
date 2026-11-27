# Overnight run — 8 hours, parallel sessions (2026-11-27 22:40 IST → 06:40 IST)

Owner is resting until ~08:00 IST. Target: **significant, verified progress**, with
the UI on screen as the headline item. Work autonomously; never idle; park a
blocker instead of stopping the whole run.

## Ground rules for every session

1. **Territory only.** Touch the files listed for your session. If a change needs
   another territory, write the request in your report instead of editing.
2. **Never `git add -A`.** Commit with `git commit --only -- <paths>`; the
   worktree is shared with other sessions.
3. **One commit + push per session round**, at the end, when your work is as
   verified as it can be on this host. Do not push half-finished experiments.
4. **Verify what you claim.** macOS headless tests are available; Linux binaries
   are not (the Dell currently boots AetherOS, so the Ubuntu toolchain is
   offline). CI (`ci.yml`) runs on every push and is the Linux check.
5. **Evidence:** write one D6 file per session at
   `docs/2026/11/27/results/<your-session>-<topic>.md` with commands and results.
   Do not edit `03-roadmap-plan.md` (S1 owns the tracker) or other sessions' files.
6. **Hardware is live but read-only.** AetherOS runs at `192.168.68.150`
   (`ssh -i ~/.ssh/id_ed25519 root@192.168.68.150`, host key changes each boot,
   use `StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null`). Do not reboot
   it, do not write to its disks, do not run the suspend test.
7. **Budget:** at most 2 retries per failing step, then record and move on.

## Workstreams

| Session | Territory | Goal |
| --- | --- | --- |
| `ui-image` | `os-build/buildroot/aetheros_defconfig`, new `os-build/buildroot/ui/`, `os-build/post-build.sh`, `os-build/board/aetheros/overlay/etc/systemd/system/`, `os-build/scripts/check-ui-image.sh` | Build the workspace binaries into the image and start the shell on tty1 (getty on tty2). Add fontconfig/freetype/xkbcommon/fonts. Verify with a pre-build script + the CI image build. |
| `kernel-input` | `os-build/kernel/aether.fragment`, `os-build/scripts/`, new docs | Fix `regulatory.db` load failure (world regulatory domain) and the touchpad not binding to i2c-hid; add `usb.ids`; investigate branding (report only). |
| `compositor` | `compositor/**` | Layer-surface damage translation (S2 residual), stale-strip check, HiDPI review; keep 73 lib + 14 integration green and `kms-checklist.sh --selftest` passing. |
| `shell` | `shell/**`, `ui/**` | End-to-end headless handshake: shell host connects to a real compositor socket and creates its layer surfaces; fix focus/keyboard gaps found. |
| `apps` | `apps/**` | Finish the remaining P4 app gaps and make the launcher actually start an app under the host (headless), with tests. |
| `testci` | `.github/workflows/**`, `os-build/test/**`, `os-build/scripts/ci-local.sh` | CI quality: Linux parity job, no-silent-drop guard for defconfig entries, capability checks (`pidof`, `pgrep`, `script`) and the acceptance harness script. |
| `sandbox` | `apps/sandbox/**` | Harden the seccomp filter: fuzz/simulate the BPF evaluator, verify Landlock ruleset translation, close the Linux-only gaps that the arch-inversion bug revealed. |

## Sequencing

- `ui-image` is the critical path; if the Buildroot Rust recipe blocks, fall back
  to building the binaries on the runner with `cargo build --release` and copying
  them into `TARGET_DIR` from `post-build.sh`, and say so in the D6 file.
- Nobody else edits `os-build/buildroot/aetheros_defconfig` while `ui-image` runs.
- Each finished session reports a HANDOFF; S1 spawns the next round with the
  leftovers (for example on-device acceptance once the UI image builds).

## Definition of done for the run

1. A new image build is green and its log shows the workspace binaries, the
   session unit and WirePlumber present.
2. `compositor`, `shell`, `apps` and `sandbox` suites green, no new warnings.
3. CI enforces Linux parity for the classes of bug found today (silent
   dependency drops, host-dependent tests, missing applets).
4. Every session has a D6 file and a tracker note queued for S1.
