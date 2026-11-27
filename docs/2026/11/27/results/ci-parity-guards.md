# CI parity guards — make the Linux-only defects fail before an image is built

- Date: 2026-11-27 (verification continued into 2026-11-28 IST)
- Session: S5 (`session - 5 (Nov27/28)`)
- Why: four defects passed on the macOS dev host and failed on the Ubuntu
  runner, and two of them (`pgrep` missing, `script` missing) made *checks* lie
  rather than fail. Details: `docs/2026/11/27/results/linux-ci-parity-fixes.md`.
- Territory used: `.github/workflows/**`, `os-build/test/**`,
  `os-build/scripts/ci-local.sh`, new `os-build/scripts/check-*.sh`, this file.
  No app/compositor/shell sources, no `test.sh`, no roadmap plan.

## What each defect now costs

| # | Defect (2026-11-27) | Why CI missed it | Guard that now catches it |
| --- | --- | --- | --- |
| 1 | `BR2_PACKAGE_WIREPLUMBER=y` silently dropped by kconfig (no Lua) → image shipped PipeWire with no session manager | Nothing compared the defconfig's intent to the generated `.config`; Buildroot prints no warning | `os-build/scripts/check-defconfig-drop.sh` in the **buildroot-build** job, right after `make defconfig` |
| 2 | `pgrep` absent → "pipewire: not running" while pid 271 was running | The harness had no idea which tools the image has, and a missing tool looks like a negative result | `os-build/test/capabilities.sh` + `os-build/test/capabilities.expected` (asserted against the built image) + `os-build/scripts/check-image-capabilities.sh` (static scan, `--strict` in `ci.yml`) |
| 3 | `script` absent → transcript capture recorded nothing | same as #2 | same as #2 (`absent script` is a manifest entry) |
| 4 | Landlock rule order, compositor client 0 (XWayland), powerd host AC adapter — all asserted something the platform does not promise | the suite ran, but a red job with a 3000-line log was unattributable; examples/benches were never exercised | `ci.yml` test step now runs `--workspace --all-targets` *and* `--doc`, with `check-test-summary.sh` publishing failing test names + captured output to the job summary and as `::error` annotations |

## Guard 1 — silent defconfig drop

**Failure mode.** In kconfig, a symbol whose prompt has unmet dependencies is
removed from `.config` **entirely**: no `=y`, no `=n`, not even
`# ... is not set`. `BR2_PACKAGE_WIREPLUMBER` requires
`BR2_PACKAGE_LUA_5_3 || BR2_PACKAGE_LUA_5_4`; with neither, the line simply does
not exist in `output/.config` and `make` succeeds. `wireplumber: not-found` only
showed up on real hardware, 40 minutes into a USB boot.

**Script.** `os-build/scripts/check-defconfig-drop.sh`

```sh
os-build/scripts/check-defconfig-drop.sh <generated/.config> [defconfig]
os-build/scripts/check-defconfig-drop.sh --self-test
```

Semantics it encodes (this is the part that needs care, because a naive "does
the string appear" check reddens on every deliberate disable):

| defconfig | `.config` | Verdict |
| --- | --- | --- |
| `SYM=y` | `SYM=y` | ok |
| `SYM=y` | line absent | **DROPPED** → exit 1 (unmet dependency, or the symbol name is wrong) |
| `SYM=y` | `# SYM is not set` | **DISABLED** → exit 1 (something forced it off) |
| `# SYM is not set` | anything | deliberate OFF in the defconfig → ignored, never reported |
| `SYM=n` | anything | deliberate OFF → ignored |
| `SYM="text"` / `SYM=123` | same value | ok |
| `SYM="text"` | different value | **CHANGED** → warning only (a requote must not redden the job) |
| `SYM="text"` | line absent | **DROPPED** → exit 1 |
| — | missing/unreadable `.config` | exit 3 — fail closed, never "pass because there was nothing to check" |

Exit codes: `0` clean, `1` drop/disable, `2` usage, `3` unreadable input. The
implementation is one portable `awk` pass (no `gawk` extensions, no bash 4
associative arrays), so the macOS host and the Ubuntu runner run the same code.

**CI step** (`.github/workflows/buildroot-build.yml`, immediately after
`make defconfig`):

```yaml
      - name: Defconfig drop guard — every requested symbol must survive
        run: |
          bash os-build/scripts/check-defconfig-drop.sh \
            os-build/buildroot/buildroot/output/.config \
            os-build/buildroot/aetheros_defconfig
```

`.github/workflows/ci.yml` runs `--self-test` on every push (no Buildroot
needed), so a regression in the guard itself is caught in 40 seconds.

**Local verification** (macOS, no Buildroot):

```
$ os-build/scripts/check-defconfig-drop.sh --self-test
check-defconfig-drop --self-test: OK (8 cases + shipped-defconfig sanity)

$ os-build/scripts/check-defconfig-drop.sh os-build/buildroot/aetheros_defconfig
check-defconfig-drop: OK — 81 requested symbol(s) survived, 3 deliberate "is not set" entries ignored.

$ grep -v '^BR2_PACKAGE_WIREPLUMBER=y' os-build/buildroot/aetheros_defconfig > /tmp/dropped.config
$ os-build/scripts/check-defconfig-drop.sh /tmp/dropped.config
DROPPED (asked for in the defconfig, gone from .config — unmet dependency or unknown symbol):
  - BR2_PACKAGE_WIREPLUMBER (asked for "y", symbol absent from .config)
check-defconfig-drop: FAIL — 1 requested symbol(s) did not survive; the image would build without them.
  Fix the dependency in the defconfig (os-build/buildroot/aetheros_defconfig) or drop the line.
[exit 1]

$ sed 's/^BR2_PACKAGE_WIREPLUMBER=y$/# BR2_PACKAGE_WIREPLUMBER is not set/' \
    os-build/buildroot/aetheros_defconfig > /tmp/disabled.config
$ os-build/scripts/check-defconfig-drop.sh /tmp/disabled.config
DISABLED (asked for in the defconfig, .config turned it off):
  - BR2_PACKAGE_WIREPLUMBER (asked for "y", .config has "# BR2_PACKAGE_WIREPLUMBER is not set")
check-defconfig-drop: FAIL — 1 requested symbol(s) did not survive; …
[exit 1]
```

Self-test cases: absent-symbol drop, deliberate `is not set` (stays quiet),
`is not set` in `.config` (fatal), clean defconfig, typo symbol, value-change
warning, missing-`.config` fails closed, and the shipped defconfig parses with a
plausible intent count (81) and passes when checked against itself.

## Guard 2 — the image's tool list is a contract

**Failure mode.** Two independent lies came from one root cause: nobody knew
what the image contained.

- `pgrep pipewire || say "pipewire: not running"` — busybox is built without
  `pgrep`, so the check reported "not running" while `pidof pipewire` was 271.
- `script -q -c …` for transcript capture — busybox has no `script` applet, so
  the capture file stayed empty and the harness treated an empty transcript as a
  successful run.

A missing tool must never stand in for a negative result.

**Files.**

| File | Role |
| --- | --- |
| `os-build/test/capabilities.sh` | probe: `present`/`absent` for a curated tool list, busybox-applet enumeration, `--root DIR` (inspect an unpacked image without executing anything from it), `--json`, `--write MANIFEST`, `--check MANIFEST`, `--self-test` |
| `os-build/test/capabilities.expected` | the checked-in contract: `present <tool>` = the image must have it (missing → CI fails); `absent <tool>` = no script may rely on it |
| `os-build/scripts/check-image-capabilities.sh` | static scan of in-image scripts for unguarded calls to `absent` tools; `--strict` fails, otherwise warns |

Every `present` line in `capabilities.expected` is evidenced by the 2026-11-27
P2 run 3 log (`docs/2026/11/27/results/logs/p2-run3-aether-test.log`) or by a
command `/root/test.sh` executed successfully in that run — the file says so, and
`capabilities.sh --write` exists so the next image run can correct it from
evidence instead of memory.

`absent` entries are the point of the exercise: `absent pgrep`, `absent script`.

**CI steps.**

`ci.yml` (every push, no image needed):

```yaml
      - name: Image capability contract (self-test, then static scan)
        run: |
          bash -n os-build/test/capabilities.sh
          bash os-build/test/capabilities.sh --self-test
          bash os-build/scripts/check-image-capabilities.sh --self-test
          bash os-build/scripts/check-image-capabilities.sh --strict
```

`buildroot-build.yml` (after `make`, on the real image — no boot required):

```yaml
      - name: Image capability contract — assert the tool list (P2 lesson)
        run: |
          ROOT=/tmp/aether-rootfs
          rm -rf "$ROOT"
          unsquashfs -no-progress -d "$ROOT" \
            os-build/buildroot/buildroot/output/images/rootfs.squashfs > /dev/null
          bash os-build/test/capabilities.sh --root "$ROOT" --check os-build/test/capabilities.expected
          bash os-build/test/capabilities.sh --root "$ROOT" > capabilities-actual.txt
          bash os-build/test/capabilities.sh --root "$ROOT" --json > capabilities-actual.json
          bash os-build/test/capabilities.sh --root "$ROOT" --applets > capabilities-applets.txt
          # … and the full probe is appended to $GITHUB_STEP_SUMMARY
```

The full probe and the applet list are uploaded as artifacts
(`capabilities-actual.txt`, `capabilities-actual.json`,
`capabilities-applets.txt`), so the first run produces the authoritative list.
`--root` never executes anything from the image, so the macOS host can use it
too.

**How a script declares a tool.** Either it uses a tool from `present` (no
ceremony), or it guards with `command -v` on the same or previous line:

```sh
if command -v glxinfo >/dev/null 2>&1; then glxinfo -B; fi
```

or waives explicitly, with a reason:

```sh
pkill -f "wpa_supplicant.*$IFACE" 2>/dev/null   # capabilities-ok: pkill: optional cleanup
```

**Local verification** (macOS):

```
$ os-build/test/capabilities.sh --self-test
capabilities --self-test: OK (10 cases)

$ os-build/scripts/check-image-capabilities.sh --self-test
check-image-capabilities --self-test: OK (10 cases)

$ os-build/scripts/check-image-capabilities.sh --strict
check-image-capabilities: OK — no unguarded use of image-absent tools in 3 file(s) [pgrep script]
  (the file count is whatever the rootfs overlay contains; it grows as scripts are added)

$ os-build/test/capabilities.sh --root <fake-root> --check /tmp/m1   # pgrep required, absent
capabilities: FAIL — 1 of 3 manifest entries are not satisfied:
  - pgrep: required by the manifest, not in the image
[exit 1]
```

Self-test cases: busybox applet recognised as `busybox` (not as a binary), a
package binary found in `/usr/bin`, `pgrep`/`script` reported absent, applet
enumeration without execution, missing-required-tool fails, satisfied manifest
passes, `absent`-but-present only notes, `--write`→`--check` round-trip,
malformed manifest exits 2, missing manifest exits 3, and the checked-in
contract really declares `pgrep`/`script` absent.

**Finding while building this guard** (reported, not fixed — `test.sh` is out of
this session's territory):

```
$ os-build/scripts/check-image-capabilities.sh --also pkill \
    --paths os-build/board/aetheros/overlay os-build/test
os-build/board/aetheros/overlay/root/test.sh:280: pkill:     pkill -f "wpa_supplicant.*$IFACE" 2>/dev/null
```

`pkill` is very likely absent for the same reason `pgrep` is (both are busybox
`PROCPS`-style applets). If so, that line is a **silent no-op** — the stale
`wpa_supplicant` is never stopped, and the new instance's failure is hidden by
`2>/dev/null`. It is left out of the `absent` list until the image proves it,
with a `TODO(verify on the next image build)` note in `capabilities.expected`,
because inventing a manifest line would be exactly the kind of unverified claim
this guard exists to prevent. Once confirmed, add `absent pkill` and fix or
waive line 280.

## Guard 3 — Linux parity job and a summary that names failures

**`ci.yml` changes.**

1. The test step runs `cargo test --workspace --all-targets` **and**
   `cargo test --workspace --doc`. `--all-targets` is wider than the old
   `--workspace` (it compiles examples and benches); it also *excludes*
   doctests, which the old command ran, hence the second pass. Both logs go to
   one file and the step exits with the first non-zero status after both ran.
2. A new step, `if: always()`, runs `os-build/scripts/check-test-summary.sh`,
   which writes GitHub-flavoured markdown to `$GITHUB_STEP_SUMMARY`: verdict,
   totals, every failing test name, its captured output, and a per-target table.
   It also emits `::error title=Test failed::<name>` annotations (stderr), so the
   failure appears on the run itself, not only inside the log.
3. `check-test-summary.sh` fails closed: a log with no `test result:` line (run
   killed, OOM, timeout) exits 4 instead of quietly "passing".
4. A `Kernel feature probe (Landlock / seccomp)` step prints the runner's
   Landlock probe JSON and `/proc/self/status` seccomp line, so a sandbox test
   that skips for lack of the ABI is distinguishable from one that passed.
5. The two guard self-tests (defconfig, capabilities) run on every push.

**`ci-local.sh` parity.** The script now runs the same gates as `ci.yml`:
build, `--all-targets` + `--doc` with the same summary parser, budgets, both
guard self-tests, the static capability scan, the S4 harness/contract block
(verbatim), and the compositor smoke test. Gates that genuinely cannot run on
the host print an explicit skip with the reason and never count as passes:

```
=== compositor smoke test: SKIPPED ===
    reason: needs Linux (XWayland, EGL, XDG_RUNTIME_DIR); this host is Darwin. …
=== capabilities contract (unpacked image): SKIPPED ===
    reason: no rootfs.squashfs (or no unsquashfs) on this host; buildroot-build.yml runs it against the built image
```

`--fast` skips the slow/auxiliary gates; `--test-only` runs build + tests +
summary only.

## Local verification: `cargo test --workspace` on macOS

The shared worktree was being edited by other parallel sessions while this ran
(`apps/terminal/src/session.rs` did not compile at one point: `cannot find type
ExitStatus`, `Copy` derive on an enum holding a `String`). To get a verdict that
belongs to committed code rather than to someone's half-finished edit, the suite
was run in a detached worktree at `HEAD = f919195`, which is what CI would test:

```
$ git worktree add --detach .../aetheros-head f919195
$ cd .../aetheros-head
$ LIBRARY_PATH=/opt/homebrew/lib cargo test --workspace
→ WORKSPACE_RC=0
  targets=125  passed=918  failed=0  ignored=0   (125 "test result:" lines, 0 FAILED)

$ LIBRARY_PATH=/opt/homebrew/lib cargo test --workspace --all-targets
→ ALLTARGETS_RC=0
  targets=101  passed=918  failed=0  ignored=0
```

Nothing was weakened and nothing was `#[ignore]`d: the suite is genuinely green
on the macOS dev host, exactly as it was on 2026-11-27 before the Linux run. That
is the point — macOS green is not evidence about Linux, which is why the guards
above exist instead of "run it on the Mac first".

The two host-gated suites do not run their assertions on macOS; the new CI step
prints that sentence instead of leaving a green box:

```
$ cargo test -p aether-sandbox --test landlock_enforce --test seccomp_enforce -- --nocapture
running 3 tests
skipped: Landlock unavailable on this host (LANDLOCK_UNAVAILABLE Landlock is a Linux kernel feature and this host is macos)
test shim_probe_reports_a_verdict ... ok
…
running 2 tests
skipped: seccomp/Landlock unavailable on this host
test seccomp_filter_runs_ordinary_programs_and_denies_blocked_syscalls ... ok
…
```

`os-build/scripts/ci-local.sh --test-only` was run end to end against the same
detached worktree (it calls the same `cargo test` lines and the same summary
parser as `ci.yml`). Its first run failed, on a test that is genuinely flaky —
the guard did its job before the commit was even written:

```
$ os-build/scripts/ci-local.sh --test-only          # run 1
…
**Verdict: FAIL** — 267 passed, **1 failed**, 0 ignored across 54 targets
### Failing tests (Linux parity)
- `navigation_tests::back_and_forward_track_history_in_both_directions`
### Captured output
**navigation_tests::back_and_forward_track_history_in_both_directions**
thread 'navigation_tests::…' panicked at apps/files/src/lib.rs:802:28:
called `Result::unwrap()` on an `Err` value: Io { path: "…/aether-files-nav-12187-1795805127932297000/B", … NotFound }
::error title=Test failed::navigation_tests::back_and_forward_track_history_in_both_directions
FAILED gates:
  - cargo test --workspace --all-targets (exit 101; …)
=== ci-local: FAILED ===   [exit 1]

$ os-build/scripts/ci-local.sh --test-only          # run 2, same tree
=== ci-local: all gates passed ===   [exit 0]
**Verdict: PASS** — 918 passed, 0 ignored across 126 targets
```

The two runs are the same commit; the difference is the flaky test described
below. Note what the guard produced on the failing run: the test's name, its
assertion, the IO error, and the exact path — instead of a red job.

### Defect found while verifying (reported, not fixed — app source)

`apps/files` `navigation_tests::back_and_forward_track_history_in_both_directions`
fails about 1 run in 30, with `Io { path: "…/aether-files-nav-<pid>-<nanos>", NotFound }`.
Test-side isolation bug, not a Linux/macOS issue:

```rust
let root = std::env::temp_dir().join(format!(
    "aether-files-nav-{}-{}",
    std::process::id(),
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
));
```

`as_nanos()` looks unique but the clock behind it ticks in **microseconds**
(observed values end in `000`, e.g. `1795805355777666000`), and the file suite
runs its tests in parallel threads of one process — same pid. Two tests that
call `sandbox()` inside the same microsecond therefore share one directory, and
the `let _ = std::fs::remove_dir_all(root);` at the end of the first one deletes
the second one's tree.

```
$ for i in $(seq 1 30); do cargo test -q -p aether-files --lib; done
RUN 13 FAILED:
thread 'navigation_tests::back_and_forward_track_history_in_both_directions' panicked at apps/files/src/lib.rs:797:25:
called `Result::unwrap()` on an `Err` value: Io { path: "…/aether-files-nav-26363-1795805355777666000", … NotFound }
failures=1/30
```

Fix belongs to whoever owns `apps/files` (a per-test `create_dir` with an atomic
counter, or `tempfile::TempDir`). It is left untouched here on purpose: this
session does not edit app sources, and weakening the assertion was never an
option. The practical effect on CI is a rare red job — which now prints *this*
paragraph's worth of information instead of `error: test failed`.

## Honest limits

- The defconfig guard and the capability contract are only *proven* against the
  live `make defconfig` / built image by `buildroot-build.yml`. This session can
  self-test both guards anywhere, but the first real run is the confirmation —
  the job prints the full probe and uploads it, so a wrong `present` line is a
  five-minute diff away.
- `unsquashfs` is used instead of booting the image; it checks the filesystem,
  not what busybox actually links at runtime. A tool that exists but is a broken
  symlink would still be listed `present` (the probe records `via=busybox` so it
  is visible).
- Nothing here makes macOS run Linux-only tests. It makes Linux failures
  attributable in one screen of summary instead of one hour of log.

## Commits

- `660b679` — `ci: guard the Linux-parity defect classes (defconfig drops, image
  capabilities, test summary)`. Ten files: the two workflows, three new
  `check-*.sh` scripts, `os-build/test/capabilities.{sh,expected}`,
  `ci-local.sh`, the `os-build/scripts/README.md` table, and this document.
  `os-build/scripts/check-ui-image.sh` (another session's untracked file) is
  deliberately not in it; the commit used `git commit --only -- <paths>`.

## Follow-ups for other owners

1. **`apps/files` flaky test** (owner: app source session) — the parallel-temp-dir
   collision described above. ~1 failure in 30 runs.
2. **`test.sh:280` `pkill`** (owner: acceptance harness / S1) — confirm with
   `os-build/test/capabilities.sh --root <rootfs>` whether the image has
   `pkill`; if not, either guard it (`command -v pkill`) or waive it with
   `# capabilities-ok: pkill: optional cleanup`, and add `absent pkill` to
   `capabilities.expected`.
3. **First `buildroot-build` run** — read `capabilities-actual.txt` from the
   artifacts and correct any `present` line the image disagrees with; that diff
   is the whole point of the contract.
