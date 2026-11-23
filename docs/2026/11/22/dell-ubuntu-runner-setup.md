# Dell Ubuntu Self-Hosted Runner — Setup & Operations Guide

- Date: 2026-11-22
- Owner: Anik
- Purpose: Run AetherOS CI on the spare Dell (Linux) for free, triggered automatically by
  `git push` from the Mac dev host. This document captures the setup we performed and the
  day-to-day operations (monitoring, troubleshooting).

## 1. Why Ubuntu on the Dell
- Every CI job we run (Buildroot build, QEMU boot tests, graphics/Wine spikes, kernel-config)
  needs Linux. Windows on the Dell is only needed later (P6 Windows app compat, P8 installer
  testing) and stays available via dual-boot.
- Ubuntu 22.04 matches the `ubuntu-22.04` runner pin and avoids the gcc C23 issue.
- The Dell boots Ubuntu by default (GRUB default = Ubuntu, short timeout) so the runner
  starts without manual intervention.

## 2. One-time setup (performed once)

### 2.1 Dual-boot Ubuntu alongside Windows
1. Back up Windows.
2. Disable Fast Startup + BitLocker in Windows.
3. Shrink the Windows partition (~80–120 GB free).
4. Boot Ubuntu 22.04 USB → "Install alongside Windows".
5. Set Ubuntu as default boot: `sudo grub-set-default 0`, edit `/etc/default/grub`
   (`GRUB_DEFAULT=0`, `GRUB_TIMEOUT=3`), then `sudo update-grub`.

### 2.2 Install build/test dependencies
```bash
sudo apt update
sudo apt install -y build-essential libncurses-dev rsync wget file bc git \
  libelf-dev mtools qemu-system-x86 expect ovmf grub-pc-bin \
  grub-efi-amd64-bin grub-common parted python3 flex bison
# graphics/Wine spikes:
sudo apt install -y libxkbcommon-dev pkg-config libfontconfig1-dev libfreetype-dev \
  wine64 gcc-mingw-w64-x86-64
```

### 2.3 Install the GitHub self-hosted runner
```bash
mkdir -p ~/actions-runner && cd ~/actions-runner
# download tarball from repo Settings -> Actions -> Runners -> New (Linux x64)
tar xzf actions-runner-linux-x64.tar.gz

# obtain a registration token (valid 1 hour) — see section 6
./config.sh --url https://github.com/Anikchan/AetherOS --token <TOKEN> \
  --name dell-test-runner --labels self-hosted,dell,linux,x64
```

### 2.4 Run as a systemd service (auto-start on boot)
```bash
sudo ./svc.sh install runner
sudo ./svc.sh start
sudo ./svc.sh status          # should show "active (running)"
sudo systemctl is-enabled actions.runner.*   # should print "enabled"
```
With this, the runner starts automatically on every reboot. You do **not** need to run
`./run.sh` manually.

## 3. Day-to-day usage

- **Mac:** edit, commit, push. Fast local checks: `cargo build/test`, `os-build/scripts/ci-local.sh`,
  `os-build/scripts/type-check.sh`, `os-build/scripts/kernel-config-check.sh`.
- **Trigger from Mac without pushing:** `gh workflow run buildroot-build.yml` (or other workflow).
- **Local run on the Dell over SSH:** `ssh dell "cd ~/AetherOS && git pull && bash os-build/scripts/ci-local.sh"`.
- **Bare-metal AetherOS test (P2 milestones):** boot Dell from USB stick with the AetherOS
  live image (F12 boot menu), run hardware checklist + `coverage-report.sh`, then reboot to Ubuntu.

## 4. Monitoring / tracking jobs

### 4.1 GitHub web UI
- Repo → **Actions** tab → shows all runs, status (queued/in progress/completed), logs, artifacts.
- Repo → **Settings → Actions → Runners** → shows runner state: Idle / Active / Offline.

### 4.2 GitHub CLI (from Mac)
```bash
# list recent runs
gh run list --limit 15

# filter by status
gh run list --status in_progress
gh run list --status queued
gh run list --status completed

# watch a specific run until it finishes
gh run watch <RUN_ID> --exit-status

# view steps / failed logs
gh run view <RUN_ID>
gh run view <RUN_ID> --log-failed

# per-job step status
gh api repos/Anikchan/AetherOS/actions/runs/<RUN_ID>/jobs \
  --jq '.jobs[0].steps[] | "\(.name): \(.status) \(.conclusion // "")"'
```

### 4.3 On the Dell itself
```bash
sudo ./svc.sh status
sudo journalctl -u 'actions.runner.*' -f     # live runner log
sudo systemctl status actions.runner.*       # service state
htop / top                                    # CPU/disk while a build runs
```

### 4.4 Notifications
- `ci.yml` has a `notify-on-failure` job that posts to Discord/Slack if the secrets
  `DISCORD_WEBHOOK` / `SLACK_WEBHOOK` are set (Repo → Settings → Secrets and variables → Actions).
- Build logs and image artifacts are uploaded with 7-day retention (`actions/upload-artifact`).

## 5. Service operations cheat sheet
```bash
sudo ./svc.sh start        # start runner
sudo ./svc.sh stop         # stop runner
sudo ./svc.sh status       # status
sudo ./svc.sh restart      # restart
sudo ./svc.sh uninstall    # remove service
sudo systemctl enable actions.runner.*    # ensure auto-start on boot
sudo systemctl is-enabled actions.runner.*  # verify enabled
```

## 6. Obtaining the registration token
The token used in `config.sh` is short-lived (1 hour). Get it via:

- **Web UI:** Settings → Actions → Runners → "New self-hosted runner" → copy the
  `--token ...` from the displayed command.
- **CLI (from Mac):**
  ```bash
  gh api repos/Anikchan/AetherOS/actions/runners/registration-token --method POST --jq '.token'
  ```
- **REST API with a PAT:** POST to
  `https://api.github.com/repos/Anikchan/AetherOS/actions/runners/registration-token`
  with `Authorization: token <PAT>`; read `.token` from the JSON response.

Generate it immediately before running `config.sh`. After registration the runner uses its
own long-lived credentials internally.

## 7. Troubleshooting
- **Runner shows Offline:** `sudo ./svc.sh status`; check network; restart service.
- **Job stuck in "queued":** no runner with matching labels available, or runner offline.
  Verify labels: `self-hosted,dell,linux,x64`.
- **Permission denied on build artifacts (e.g. QEMU can't open image):** ensure
  `make-bios-image.sh` chmods/chowns the image (already fixed in 3b3bff9/ebea603).
- **Build hangs:** the Build step has a 90-min timeout (ci(buildroot)); check runner disk space.
- **Runner not auto-starting after reboot:** `sudo systemctl enable actions.runner.*`.

## 8. Notes
- The runner and bare-metal AetherOS tests are mutually exclusive on one machine; schedule
  bare-metal tests off-hours.
- For a public repo, fork-PR workflows can run arbitrary code on the runner — keep the
  repo private or disable fork workflow runs on self-hosted runners.
- Keep Windows available via dual-boot/USB for P6 (Windows app compat) and P8 (installer) testing.