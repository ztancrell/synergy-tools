# synergy-tools phase 3 (install, autostart, release) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `synergy-tools install` / `synergy-tools uninstall` on Linux, Windows and macOS (per-user copy, login autostart, right-click / Finder "Send to other PC", removal of the old `synergy-audio` / `synergy-dragndrop` setups in the spec's order), and one release workflow on `v*` tags that publishes the Linux, Windows and macOS universal binaries.

**Architecture:** One new module, `src/install.rs`. Each OS's install/uninstall is a pure function that returns a list of `Step`s (run a command, delete a path, write a file, copy the exe, edit the Openbox autostart file); one `apply` executes any list. The lists, the generated file texts and `apply` itself are unit-tested on Linux in temp dirs with a stub command runner; the real OS calls are covered by the three CI jobs and the hands-on list. CI gains `workflow_call`; a new `release.yml` calls it and attaches its artifacts to a GitHub Release.

**Tech Stack:** Rust 2021, std + `dirs` (already a dependency). OS tools only: Linux `systemctl`, `pkill`; Windows `reg`, `schtasks`, `taskkill`, `powershell` (WScript.Shell shortcut, NetFirewallRule); macOS `codesign`, `xattr`, `pbs`, `open`, `pkill`, `plutil` (CI test only). GitHub Actions `actions/checkout@v7`, `actions/upload-artifact@v7`, `actions/download-artifact@v8`, `gh release create`. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-05-unified-tray-design.md` (goal 7; "Calls made by Claude": Migration, Linux autostart; "Left to the builder": CI/release; "Decided"). Earlier plans: `docs/superpowers/plans/2026-11-05-synergy-tools-phase1.md`, `...-phase2.md`.

## Global Constraints

- Binary `synergy-tools` (`synergy-tools.exe`), version from `Cargo.toml` (`0.1.0`). Ports unchanged: UDP 24850, TCP 24851. Config dir `dirs::config_dir()/synergy-tools/` is never touched by install or uninstall (pairings survive).
- Windows removal order is the spec's, exactly: run the old `synergy-audio.exe unmute` if it exists; stop and unregister scheduled tasks `synergy-audio` and `synergy-dragndrop` and kill those processes; delete `%LOCALAPPDATA%\synergy-audio`, `%LOCALAPPDATA%\synergy-dragndrop`, `%APPDATA%\synergy-audio`, `%APPDATA%\synergy-dragndrop` and `%APPDATA%\Microsoft\Windows\SendTo\Send to other PC.lnk`; remove firewall rule `synergy-dragndrop (TCP 24851)` (needs admin).
- Linux autostart "= launch the tray at desktop login, which replaces the systemd receiver service". Headless `audio receive` is documented, "not installed by default; it must not run at the same time as the tray copy".
- Install never fails because of an admin-only step; it prints one command to run as administrator instead.
- `install` is idempotent; `uninstall` removes only what `install` added (not the old programs' leftovers, not the config dir).
- Release: one tag scheme, `v*`. No signing, no notarization, no MSI/pkg/deb.
- No new crates. Anti-goals still bind: one tray icon, one pairing, CLI keeps working.
- Docs change in the same commit as the code they describe (README.md, docs/files.md, docs/audio.md). Notes that say "phase 3 will…" are replaced by current text; real history keeps a one-line `Superseded:` note pointing at the current code.
- Tests: each step runs one filter (`cargo test install::`). The full suite runs once, in Task 5. No Wine/Proton.
- Every task ends with `cargo build` showing zero warnings, commit, `git push origin phase3`, then `id=$(gh run list --branch phase3 --limit 1 --json databaseId -q '.[0].databaseId'); gh run watch "$id" --exit-status` (wait a few seconds after the push so the run exists) — linux, windows and macos green (CI has `RUSTFLAGS=-D warnings`).
- The controller is Linux: Windows and macOS code is compiled only by CI.

## Calls made by Claude (user: check these)

1. **Installer = a subcommand** (`synergy-tools install` / `uninstall`), no MSI/pkg/deb. Install: remove old programs, copy the running exe to a fixed per-user place, register login autostart, add the send entry, start the tray if none is running (TCP 24851 free), otherwise print "quit the running tray and start it again".
2. **Linux:** `~/.local/bin/synergy-tools`; autostart `~/.config/autostart/synergy-tools.desktop` (XDG). Openbox without an `openbox-xdg-autostart` helper but with `~/.config/openbox/autostart` gets a line `'<bin>' & # synergy-tools` there instead (and the XDG file is removed), the rule the old dragndrop installer used, so it never starts twice. Right-click: Thunar `sendto`, Dolphin service menu, Nautilus script, all named "Send to other PC", running `synergy-tools files send`.
3. **Windows:** `%LOCALAPPDATA%\Programs\synergy-tools\synergy-tools.exe`; autostart = `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `synergy-tools` (no admin, no scheduled task, no 10 s delay). Send To = `%APPDATA%\Microsoft\Windows\SendTo\Send to other PC.lnk` → new exe with arguments `files send` (phase 1/2 had none; the spec's cleanup deletes the old one at that same path first). One entry, not one per partner: with several online the notification names them (phase 1 decision 8 said per-partner; superseded).
4. **Windows firewall** = one admin step: remove `synergy-dragndrop (TCP 24851)` and add `synergy-tools (TCP 24851)` (inbound TCP 24851, Private+Public, for the installed exe), so files and presence still reach this PC. Tried without elevation (no UAC prompt); on failure install prints `powershell -NoProfile -NonInteractive -Command "…"` to run as administrator and succeeds.
5. **macOS:** a minimal app bundle `~/Applications/synergy-tools.app` (Info.plist + the binary, ad-hoc `codesign`), because at login nothing is started from Terminal, and the System Audio Recording prompt needs a bundle with `NSAudioCaptureUsageDescription` to attribute to. Autostart = LaunchAgent `~/Library/LaunchAgents/io.github.ztancrell.synergy-tools.plist` running `/usr/bin/open <app>` at load. Finder = Quick Action `~/Library/Services/Send to other PC.workflow` (Run Shell Script `'<bin>' files send "$@"`). If Finder doesn't show it in the hands-on check, the fallback is documented steps to build the same Quick Action in Automator by hand (docs/files.md), not more code. Notifications stay osascript/text-only (spec "Decided").
6. **Linux cleanup also deletes** the old binaries in `~/.local/bin`, the old right-click entries and the old config dirs `~/.config/synergy-audio`, `~/.config/synergy-dragndrop` (mirroring the spec's Windows list; their keys don't work with the new program). macOS has nothing to clean (the old sender had no autostart there).
7. **Release** = `release.yml` on `v*` tags calls `ci.yml` (`workflow_call`), then one job checks the tag equals `v` + Cargo version and runs `gh release create <tag> --generate-notes` with `synergy-tools-linux-x86_64`, `synergy-tools-windows-x86_64.exe`, `synergy-tools-macos-universal`. `workflow_dispatch` builds without publishing (how this phase tests it). There are no old `audio-v*`/`dragndrop-v*` tags or releases on the remote, so nothing to delete.
8. **CI actions:** `actions/checkout@v7`, `actions/upload-artifact@v7`, `actions/download-artifact@v8` (latest majors, Node 24). `ci.yml` push trigger limited to branches so a tag push runs only `release.yml`.

## Review Focus

- **Updating: `install` run again while the installed tray is running** (Windows locks a running exe; Linux says "text file busy" on overwrite). Expected: the copy succeeds (old file renamed to `<name>.old` first), install says to restart the tray. Test: Task 1 `copy_replaces_existing_target`; Windows lock in hands-on.
- **`install` run twice, or from the installed copy itself.** Expected: same result, no error, nothing doubled (one Openbox line, one autostart entry). Tests: Task 1 `apply_twice_is_idempotent`, `copy_onto_itself_is_noop`, `openbox_edit_adds_once_and_removes`.
- **Home paths with spaces, `'`, `$`, `&`** (`C:\Users\Zach O'Brien`, `/Users/o'b&c`). Expected: every generated entry still runs the right binary. Tests: Task 1 `desktop_quote_escapes`, `sh_quote_handles_apostrophe`; Task 2 `ps_quote_doubles_single_quotes`; Task 3 `workflow_doc_quotes_path`.
- **Not an administrator on Windows.** Expected: install succeeds and ends with the one command to run as admin. Test: Task 1 `admin_failure_is_a_note_not_an_error`; Task 2 `firewall_ps_has_no_double_quotes`.
- **`uninstall` from the installed exe on Windows** (it can't delete itself). Expected: everything else removed, then "delete <folder> by hand". Test: Task 1 `failed_delete_is_a_note`; hands-on.

---

## Task order

| Task | Depends on | Notes |
|------|------------|-------|
| 1 `install.rs` core and Linux | none | CLI `install`/`uninstall`, all Linux behaviour |
| 2 Windows | 1 | |
| 3 macOS | 1 | after 2 (same file) |
| 4 Release workflow, CI action bumps | none | can run anytime; sequential with the others for one agent |
| 5 Full suite, docs sweep, hands-on | 1-4 | |

---

### Task 1: `install.rs` core and Linux

**Files:**
- Create: `src/install.rs`
- Modify: `src/main.rs` (mod, USAGE, two match arms), `README.md`, `docs/files.md`, `docs/audio.md`

**Interfaces:**
- Produces (in `src/install.rs`):
  ```rust
  #[derive(Debug, PartialEq)]
  pub enum Step {
      Try(Vec<String>),                 // old-program cleanup: failure ignored
      Run(Vec<String>),                 // failure fails install/uninstall
      Admin(Vec<String>),               // failure -> note: the command to run as administrator
      Delete(PathBuf),                  // file or folder; missing is fine; failure -> note
      Write(PathBuf, String),           // parents created, replaces
      WriteExec(PathBuf, String),       // same, mode 0o755 on unix
      Copy(PathBuf, PathBuf),           // exe -> place, see Step 3
      Openbox(PathBuf, &'static str, Option<String>), // autostart file, marker, line to add; skipped if the file is missing
  }
  pub struct Places { pub exe: PathBuf, pub home: PathBuf, pub config: PathBuf, pub data: PathBuf }
  // exe = current_exe; home/config/data = dirs::{home_dir, config_dir, data_dir}; Task 2 adds `local`
  pub fn apply(steps: &[Step], run: &mut dyn FnMut(&[String]) -> bool) -> Result<Vec<String>, String> // Ok(notes for the user)
  pub fn openbox_edit(text: &str, marker: &str, add: Option<&str>) -> String
  pub fn linux(p: &Places, install: bool, openbox: bool) -> Vec<Step>
  pub fn installed_exe(p: &Places) -> PathBuf          // per cfg OS; Linux: home/.local/bin/synergy-tools
  pub fn install() -> Result<(), String>
  pub fn uninstall() -> Result<(), String>
  fn sh_quote(s: &str) -> String       // '…' with ' -> '\''
  fn desktop_quote(p: &Path) -> String // "…", and each of " ` $ \ written as \\ + char (Desktop Entry spec: quoting, then string escaping)
  ```
- `main.rs`: `["install"] => install::install()?`, `["uninstall"] => install::uninstall()?`. USAGE gains:
  ```
    synergy-tools install                 copy to a per-user folder, start at login, add the "Send to other PC"
                                          entry, remove the old synergy-audio/synergy-dragndrop
    synergy-tools uninstall               undo install (pairings are kept)
  ```
- `linux(p, true, openbox)` = cleanup, then adds. With `bin = home/.local/bin/synergy-tools`, `ob = config/openbox/autostart`, `xdg = config/autostart/synergy-tools.desktop`:
  - cleanup, in this order: `Try(systemctl --user disable --now synergy-audio.service)`, `Try(pkill -f <home>/.local/bin/synergy-audio)`, `Try(pkill -f <home>/.local/bin/synergy-dragndrop)`, `Delete(config/systemd/user/synergy-audio.service)`, `Try(systemctl --user daemon-reload)`, `Delete(config/autostart/synergy-dragndrop.desktop)`, `Openbox(ob, "synergy-dragndrop", None)`, `Delete(data/Thunar/sendto/synergy-dragndrop.desktop)`, `Delete(data/kio/servicemenus/synergy-dragndrop.desktop)`, `Delete(data/nautilus/scripts/Send to other PC)`, `Delete(home/.local/bin/synergy-audio)`, `Delete(home/.local/bin/synergy-dragndrop)`, `Delete(config/synergy-audio)`, `Delete(config/synergy-dragndrop)`.
  - adds: `Copy(exe, bin)`; openbox=false: `Openbox(ob, "synergy-tools", None)`, `Write(xdg, …)`; openbox=true: `Delete(xdg)`, `Openbox(ob, "synergy-tools", Some(sh_quote(bin) + " & # synergy-tools"))`; `Write(data/Thunar/sendto/synergy-tools.desktop)`, `WriteExec(data/kio/servicemenus/synergy-tools.desktop)` (Plasma 6 needs +x), `WriteExec(data/nautilus/scripts/Send to other PC)`.
  - `linux(p, false, _)`: `Try(pkill -x -f <bin>)` (exact cmdline = the tray, never the uninstall itself), `Delete(bin)`, `Delete(xdg)`, `Openbox(ob, "synergy-tools", None)`, `Delete` of the three right-click files.
- File texts (Exec = `desktop_quote(bin)`): xdg = `[Desktop Entry]`, `Type=Application`, `Name=synergy-tools`, `Comment=Audio and files to your other PC`, `Exec=<q>`, `Terminal=false`, `X-GNOME-Autostart-enabled=true`. Thunar = `Type=Application`, `Name=Send to other PC`, `Icon=document-send`, `Exec=<q> files send %F`, `MimeType=application/octet-stream;`. Dolphin = `Type=Service`, `MimeType=application/octet-stream;`, `X-KDE-ServiceTypes=KonqPopupMenu/Plugin`, `Actions=send`, `[Desktop Action send]`, `Name=Send to other PC`, `Icon=document-send`, `Exec=<q> files send %F`. Nautilus = `#!/bin/sh\nexec <sh_quote(bin)> files send "$@"\n`.
- `install()` (Linux): openbox = `ob` exists and none of `/usr/local/libexec/openbox-xdg-autostart`, `/usr/libexec/openbox-xdg-autostart`, `/usr/lib/openbox/openbox-xdg-autostart`, `/usr/bin/openbox-xdg-autostart` exists. Real runner: `Command` with null stdio, `true` on exit 0. Prints what was installed (exe path, autostart, send entry), then the notes, then starts the tray: if `TcpListener::bind(("0.0.0.0", crate::PORT))` succeeds, drop it and spawn `installed_exe` detached with null stdio ("started"); else print `a synergy-tools tray is already running: Quit it from its menu and start <path> to use this version`.

- [ ] **Step 1: Write the failing tests** in `install::tests` (temp dirs like `config::tests::tmp`):
  - `openbox_edit_adds_once_and_removes`: `openbox_edit("tint2 &\n", "synergy-tools", Some(L))` == `"tint2 &\n" + L + "\n"`; editing that result again with the same args returns it unchanged; `openbox_edit(that, "synergy-tools", None)` == `"tint2 &\n"`; `openbox_edit("tint2 &", "synergy-tools", Some(L))` == `"tint2 &\n" + L + "\n"`; `openbox_edit("/home/u/.local/bin/synergy-dragndrop & # synergy-dragndrop\ntint2 &\n", "synergy-dragndrop", None)` == `"tint2 &\n"`.
  - `desktop_quote_escapes`: `/home/a b/x$y` → `"/home/a b/x\\$y"` (the file holds two backslashes).
  - `sh_quote_handles_apostrophe`: `/home/o'b/x` → `'/home/o'\''b/x'`.
  - `linux_cleanup_comes_first_in_order`: the first 14 steps of `linux(p, true, false)` equal the cleanup list above, exactly.
  - `linux_autostart_xdg_or_openbox`: openbox=false has `Write(xdg, t)` with `t` containing `Exec="<bin>"` and no `Delete(xdg)`; openbox=true has `Delete(xdg)` and `Openbox(ob, "synergy-tools", Some(format!("'{bin}' & # synergy-tools")))` and no `Write(xdg, _)`.
  - `uninstall_reverses_install`: every path in a `Write`/`WriteExec`/`Copy` target of `linux(p, true, b)` is a `Delete` in `linux(p, false, b)` and vice versa, for b in {false, true} (`Delete(xdg)` in the openbox=true install is the one allowed extra).
  - `apply_twice_is_idempotent`: temp `Places` with a 3-byte fake `exe`; pre-create `home/.local/bin/synergy-dragndrop`, `config/autostart/synergy-dragndrop.desktop`, `config/openbox/autostart` = `"tint2 &\n/x/synergy-dragndrop & # synergy-dragndrop\n"`; `apply(&linux(&p, true, true), stub)` twice, stub returns true and records commands → both Ok, the snapshot (relative path + bytes of every file) after run 1 equals after run 2; the old files are gone; the autostart file has exactly one `# synergy-tools` line. Then `apply(&linux(&p, false, true), stub)` → only `exe` and `config/openbox/autostart` == `"tint2 &\n"` remain as files.
  - `copy_onto_itself_is_noop`: `apply(&[Copy(a.clone(), a.clone())], stub)` → Ok, `a` unchanged.
  - `copy_replaces_existing_target` (unix): target exists with other bytes → after Copy it holds the source bytes, is mode 0o755, and no `<target>.new` / `<target>.old` is left.
  - `admin_failure_is_a_note_not_an_error`: `apply(&[Admin(vec!["x".into(), "y z".into()])], |_| false)` → `Ok(notes)` with one note containing `as administrator` and `x "y z"`. `Run` with a failing stub → `Err`. `Try` with a failing stub → `Ok(vec![])`.
  - `failed_delete_is_a_note`: Delete of a path that can't be removed (a non-empty dir made read-only on unix: `dir/sub/file`, chmod 0o500 on `dir/sub`, Delete(`dir/sub/file`)) → `Ok` with a note naming the path and `delete it by hand`; restore mode after.
- [ ] **Step 2: Run** `cargo test install::` — FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces. `apply` per step: `Try` ignore result; `Run` false → `Err("<cmd> failed")`; `Admin` false → note `Run this as administrator: <cmd joined with spaces, items containing a space in double quotes>` (Task 2 uses it with one script item); `Delete`: `remove_dir_all` for dirs else `remove_file`, NotFound is fine; `Copy(from, to)`: if both canonicalize to the same path → skip; else create parents, remove `<to>.old` (ignore), rename `to` → `<to>.old` if it exists, copy `from` → `<to>.new`, set 0o755 on unix, rename to `to`, try to remove `<to>.old` (ignore; on Windows it fails while the old tray runs, the next install removes it); `Openbox`: read, `openbox_edit`, write only if changed. `installed_exe` and `install`/`uninstall` take their OS branch via `#[cfg]` (Tasks 2-3 add Windows and macOS; until then those OSes return `Err("install is not supported on this OS yet")` so CI builds warning-free). The pure step builders and text generators of every OS compile on every OS (the tests run on Linux); mark each with `#[cfg_attr(not(<its os>), allow(dead_code))]`, and an enum variant no OS uses yet (`Admin` until Task 2) with `#[allow(dead_code)]` until it is used.
- [ ] **Step 4: Run** `cargo test install::` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/files.md` "## Install": replace the "phase 3 adds an installer" note with the Linux procedure: download (release asset, Task 4) or build, `chmod +x`, `./synergy-tools install`; what it does (copy to `~/.local/bin`, XDG autostart or Openbox autostart line, three "Send to other PC" entries, removes the old synergy-audio service and synergy-dragndrop entries and binaries and their old config folders, starts the tray); run it again to update; then add `Superseded: the per-tool install-autostart.sh scripts; synergy-tools install replaces them (src/install.rs).` "Right-click" bullet: current for Thunar/Dolphin/Nautilus; Windows Explorer and Finder still "come with phase 3's installer". "## Uninstall": `synergy-tools uninstall` (Linux; keeps pairings; delete `~/.config/synergy-tools` to forget them) and its Superseded line pointing at it. `docs/audio.md`: "Download and install" → on Linux the tray from `synergy-tools install` is the receiver (no systemd service any more; install removes the old one); headless: `synergy-tools audio receive` is not installed, run it yourself (e.g. your own systemd user unit) and never together with the tray (UDP 24850); "Every update after that, autostart" → download/build, `synergy-tools install` again, Quit and restart the tray. README: tray paragraph mentions `synergy-tools install` (link to docs/files.md#install).
- [ ] **Step 6: Commit** `git commit -m "install/uninstall subcommands: Linux autostart, right-click entries, old-program cleanup"`; push; watch CI by run id — all green.

### Task 2: Windows

**Files:**
- Modify: `src/install.rs`, `README.md`, `docs/files.md`, `docs/audio.md`

**Interfaces:**
- Consumes: Task 1 `Step`, `Places`, `apply`, `installed_exe`, `install`/`uninstall`.
- Produces: `Places` gains `pub local: PathBuf` (`dirs::data_local_dir()`, `%LOCALAPPDATA%`); `pub fn windows(p: &Places, install: bool) -> Vec<Step>`, `fn ps_quote(s: &str) -> String` (`'…'`, `'` doubled), `fn ps(script: String) -> Vec<String>` = `["powershell", "-NoProfile", "-NonInteractive", "-Command", script]`, `fn firewall_ps(exe: &Path) -> String`, `fn sendto_ps(lnk: &Path, exe: &Path) -> String`.
- With `dir = local/Programs/synergy-tools`, `exe_t = dir/synergy-tools.exe`, `lnk = config/Microsoft/Windows/SendTo/Send to other PC.lnk` (`config` = `%APPDATA%` on Windows):
  - cleanup (spec order): `Try([local/synergy-audio/synergy-audio.exe, "unmute"])`; `Try(schtasks /End /TN synergy-audio)`, `Try(schtasks /Delete /TN synergy-audio /F)`, `Try(schtasks /End /TN synergy-dragndrop)`, `Try(schtasks /Delete /TN synergy-dragndrop /F)`, `Try(taskkill /F /IM synergy-audio.exe)`, `Try(taskkill /F /IM synergy-dragndrop.exe)`; `Delete` of `local/synergy-audio`, `local/synergy-dragndrop`, `config/synergy-audio`, `config/synergy-dragndrop`, `lnk`; `Admin(ps(firewall_ps(exe_t)))`.
  - adds: `Copy(exe, exe_t)`, `Run(reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v synergy-tools /t REG_SZ /d "\"<exe_t>\"" /f)`, `Run(ps(sendto_ps(lnk, exe_t)))`.
  - uninstall: `Try(taskkill /F /FI "IMAGENAME eq synergy-tools.exe" /FI "PID ne <std::process::id()>")`, `Try(reg delete …\Run /v synergy-tools /f)`, `Delete(lnk)`, `Admin(ps("$ErrorActionPreference='Stop'; Get-NetFirewallRule -DisplayName 'synergy-tools (TCP 24851)' -ErrorAction SilentlyContinue | Remove-NetFirewallRule"))`, `Delete(dir)`.
- `firewall_ps`: starts `$ErrorActionPreference='Stop'; `, removes `'synergy-dragndrop (TCP 24851)'` via `Get-NetFirewallRule … -ErrorAction SilentlyContinue | Remove-NetFirewallRule`, then if `'synergy-tools (TCP 24851)'` is missing `New-NetFirewallRule -DisplayName 'synergy-tools (TCP 24851)' -Direction Inbound -Protocol TCP -LocalPort 24851 -Program <ps_quote(exe)> -Profile Private,Public -Action Allow | Out-Null`. Never contains `"`.
- `sendto_ps`: `$s=(New-Object -ComObject WScript.Shell).CreateShortcut(<ps_quote(lnk)>); $s.TargetPath=<ps_quote(exe)>; $s.Arguments='files send'; $s.Save()`.
- After `Copy` on Windows, `apply` also removes `<to>:Zone.Identifier` (ignore errors) so a downloaded exe doesn't carry the Mark of the Web into the install.
- Start after install: spawn `exe_t` (GUI subsystem, no console). `installed_exe` Windows branch = `exe_t`.

- [ ] **Step 1: Write the failing tests.** `windows_cleanup_in_spec_order`: the first 13 steps of `windows(p, true)` equal the cleanup list above, exactly. `windows_adds`: the steps after it are `Copy(p.exe, exe_t)`, the `reg add` Run whose `/d` item is `"\"" + exe_t + "\""`, and a `Run` whose script contains `Send to other PC.lnk`, `$s.Arguments='files send'` and `TargetPath='<exe_t>'`. `ps_quote_doubles_single_quotes`: `C:\Users\Zach O'Brien\x.exe` → `'C:\Users\Zach O''Brien\x.exe'`. `firewall_ps_has_no_double_quotes`: for that path, no `"`, contains `-LocalPort 24851`, `-Profile Private,Public`, `'synergy-dragndrop (TCP 24851)'`, `'synergy-tools (TCP 24851)'`, `Remove-NetFirewallRule` before `New-NetFirewallRule`. Extend `uninstall_reverses_install` to Windows: `Copy` target lies inside a deleted `dir`; `lnk` is deleted; Run key added ↔ deleted.
- [ ] **Step 2: Run** `cargo test install::` — FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces and the `#[cfg(windows)]` branches of `installed_exe`, `install`, `uninstall`.
- [ ] **Step 4: Run** `cargo test install::` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/files.md` Install, Windows: download `synergy-tools-windows-x86_64.exe`, in PowerShell `.\synergy-tools-windows-x86_64.exe install` (SmartScreen: More info > Run anyway); what it does (the spec's removal list in order, `%LOCALAPPDATA%\Programs\synergy-tools`, Run key, Send To "Send to other PC", firewall rule or the printed admin command); Right-click bullet now covers Windows Explorer; Uninstall covers Windows (from the installed exe the folder stays: delete it by hand). Troubleshooting "unreachable": the firewall line names the rule install adds. `docs/audio.md`: Windows install = same command; old scheduled task `synergy-audio` is removed by it (after `unmute`).
- [ ] **Step 6: Commit** `git commit -m "install on Windows: spec-order cleanup, Run key, Send To, firewall"`; push; watch CI by run id.

### Task 3: macOS

**Files:**
- Modify: `src/install.rs`, `README.md`, `docs/files.md`, `docs/audio.md`

**Interfaces:**
- Consumes: Task 1.
- Produces: `pub fn macos(p: &Places, install: bool) -> Vec<Step>`, `fn xml_escape(s: &str) -> String` (`&` `<` `>`), `fn info_plist() -> String`, `fn launch_agent(app: &Path) -> String`, `fn workflow_info_plist() -> String`, `fn workflow_doc(bin: &Path) -> String`.
- With `app = home/Applications/synergy-tools.app`, `bin = app/Contents/MacOS/synergy-tools`, `agent = home/Library/LaunchAgents/io.github.ztancrell.synergy-tools.plist`, `wf = home/Library/Services/Send to other PC.workflow`:
  - install (nothing to clean on macOS): `Copy(exe, bin)`, `Write(app/Contents/Info.plist, info_plist())`, `Try(xattr -dr com.apple.quarantine <app>)`, `Try(codesign --force --sign - <app>)`, `Write(agent, launch_agent(app))`, `Write(wf/Contents/Info.plist, workflow_info_plist())`, `Write(wf/Contents/document.wflow, workflow_doc(bin))`, `Try(/System/Library/CoreServices/pbs -update)`.
  - uninstall: `Try(pkill -x -f <bin>)`, `Delete(app)`, `Delete(agent)`, `Delete(wf)`, `Try(pbs -update)`.
- `info_plist`: `CFBundleIdentifier` `io.github.ztancrell.synergy-tools`, `CFBundleName`/`CFBundleExecutable` `synergy-tools`, `CFBundlePackageType` `APPL`, `CFBundleShortVersionString` `env!("CARGO_PKG_VERSION")`, `LSMinimumSystemVersion` `14.6`, `LSUIElement` `<true/>`, `NSAudioCaptureUsageDescription` `synergy-tools streams this Mac's sound to your Linux PC.`
- `launch_agent`: `Label` `io.github.ztancrell.synergy-tools`, `ProgramArguments` [`/usr/bin/open`, `<app>`], `RunAtLoad` `<true/>`.
- `workflow_info_plist`: `NSServices` = one dict: `NSMenuItem` {`default` = `Send to other PC`}, `NSMessage` `runWorkflowAsService`, `NSRequiredContext` {`NSApplicationIdentifier` `com.apple.finder`}, `NSSendFileTypes` [`public.item`].
- `workflow_doc`: an Automator document plist with `AMDocumentVersion` `2`, one action whose `ActionBundlePath` is `/System/Library/Automator/Run Shell Script.action`, `BundleIdentifier` `com.apple.RunShellScript`, `Class Name` `RunShellScriptAction`, `ActionParameters` {`COMMAND_STRING` = `xml_escape(sh_quote(bin) + " files send \"$@\"")`, `inputMethod` `1` (as arguments), `shell` `/bin/sh`, `source` empty}, and `workflowMetaData` {`workflowTypeIdentifier` `com.apple.Automator.servicesMenu`, `serviceInputTypeIdentifier` `com.apple.Automator.fileSystemObject`, `serviceOutputTypeIdentifier` `com.apple.Automator.nothing`, `serviceApplicationBundleID` `com.apple.finder`}. Copy the other keys from a Quick Action saved by Automator if the hands-on check shows they are needed.
- Start after install: `open <app>`. `installed_exe` macOS branch = `bin`.

- [ ] **Step 1: Write the failing tests.** `macos_steps`: `macos(p, true)` equals the install list above exactly. `info_plist_keys`: contains `<key>LSUIElement</key>` followed by `<true/>`, `io.github.ztancrell.synergy-tools`, `NSAudioCaptureUsageDescription`, `env!("CARGO_PKG_VERSION")`. `launch_agent_opens_app`: contains `/usr/bin/open`, the app path, `RunAtLoad`. `workflow_doc_quotes_path`: for bin `/Users/o'b&c/x`, contains `'/Users/o'\''b&amp;c/x' files send "$@"`, `Run Shell Script.action`, `com.apple.Automator.servicesMenu`, `com.apple.Automator.fileSystemObject`. `#[cfg(target_os = "macos")] plists_lint`: write the four generated texts to a temp dir and `plutil -lint` each → exit 0 (runs in the macos CI job). Extend `uninstall_reverses_install` to macOS (written paths lie in deleted `app` / `wf` or equal `agent`).
- [ ] **Step 2: Run** `cargo test install::` — FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces and the `#[cfg(target_os = "macos")]` branches; after this no OS returns "not supported yet".
- [ ] **Step 4: Run** `cargo test install::` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/files.md` "macOS": Gatekeeper (`xattr -d com.apple.quarantine synergy-tools-macos-universal; chmod +x synergy-tools-macos-universal; ./synergy-tools-macos-universal install`), what install makes (`~/Applications/synergy-tools.app`, LaunchAgent, macOS shows "Background item added", Finder right-click > Quick Actions > Send to other PC; if missing: System Settings > Keyboard > Keyboard Shortcuts > Services > Files and Folders, or the documented Automator steps), the CLI is `~/Applications/synergy-tools.app/Contents/MacOS/synergy-tools`; replace the "Finder send entry comes with phase 3's installer" sentence and the Right-click bullet's last "phase 3" mention; Downloads prompt now names synergy-tools. Uninstall covers macOS. `docs/audio.md` "macOS (sender)": the System Audio Recording prompt now names synergy-tools (not Terminal) when started at login; after an update macOS may ask again. 
- [ ] **Step 6: Commit** `git commit -m "install on macOS: app bundle, LaunchAgent, Finder Quick Action"`; push; watch CI by run id (macos runs `plists_lint`).

### Task 4: Release workflow, CI action bumps

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml`, `README.md`, `docs/audio.md`, `docs/files.md`

**Interfaces:**
- Produces: artifacts from `ci.yml` named `synergy-tools-linux-x86_64` (file `target/release/synergy-tools`), `synergy-tools-windows-x86_64` (`target/release/synergy-tools.exe`), `synergy-tools-macos-universal` (unchanged); release assets `synergy-tools-linux-x86_64`, `synergy-tools-windows-x86_64.exe`, `synergy-tools-macos-universal`.
- `ci.yml`: `on: push: branches: ['**']`, `pull_request`, `workflow_call`; every `actions/checkout@v4` → `@v7`, `upload-artifact@v4` → `@v7`; linux and windows jobs gain an upload step.
- `release.yml`:
  ```yaml
  name: Release
  on:
    push: { tags: ['v*'] }
    workflow_dispatch:
  jobs:
    build:
      uses: ./.github/workflows/ci.yml
    publish:
      needs: build
      if: startsWith(github.ref, 'refs/tags/v')
      runs-on: ubuntu-latest
      permissions: { contents: write }
      steps:
        - uses: actions/checkout@v7
        - run: test "${GITHUB_REF_NAME#v}" = "$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
        - uses: actions/download-artifact@v8
          with: { path: dist }
        - run: |
            mkdir out
            mv dist/synergy-tools-linux-x86_64/synergy-tools out/synergy-tools-linux-x86_64
            mv dist/synergy-tools-windows-x86_64/synergy-tools.exe out/synergy-tools-windows-x86_64.exe
            mv dist/synergy-tools-macos-universal/synergy-tools out/synergy-tools-macos-universal
            gh release create "$GITHUB_REF_NAME" --generate-notes out/*
          env: { GH_TOKEN: '${{ github.token }}' }
  ```

- [ ] **Step 1: Implement** both workflow files as above.
- [ ] **Step 2: Docs.** README "CI and releases": `ci.yml` (three jobs, warnings fail, three binaries as workflow artifacts); a release = bump `version` in `Cargo.toml`, commit, `git tag v<version> && git push origin v<version>` → `release.yml` builds and publishes the three assets (the tag must match the Cargo version); unsigned: Mac users remove the quarantine flag (`xattr -d com.apple.quarantine …`, see docs/files.md#macos), Windows users get SmartScreen (More info > Run anyway). Replace the "phase 3 adds one release workflow" note with `Superseded: per-tool tags audio-v* / dragndrop-v* and their workflows; one v* tag now (.github/workflows/release.yml).` Same in docs/audio.md "Releases and CI" / "Making a release". `docs/files.md` Status: "Not released yet" → releases on the repo's Releases page; "Build from source" CI sentence names the three artifacts.
- [ ] **Step 3: Commit** `git commit -m "Release workflow on v* tags; CI actions to checkout@v7, upload-artifact@v7"`; push; watch the `CI` run by id — green, with three artifacts (`gh api repos/{owner}/{repo}/actions/runs/<id>/artifacts -q '.artifacts[].name'` lists the three names). `release.yml` can't be dispatched before it is on `main` (GitHub only dispatches workflows known on the default branch), so its first run is in the hands-on list after the merge; here, check `gh run list --branch phase3 --limit 3` shows no "workflow file issue" failure for it. No tag is pushed in this phase.

### Task 5: Full suite, docs sweep, hands-on

**Files:** `docs/superpowers/plans/2026-11-05-synergy-tools-phase1.md` and `...-phase2.md` (Superseded lines); others only if a check fails (fix in the owning file with its docs).

- [ ] **Step 1:** In the background, `cargo test > "$SCRATCH/test.log" 2>&1; cargo build --release >> "$SCRATCH/test.log" 2>&1`; read only the `test result:` lines, failures and warnings. Expected: all pass, zero warnings.
- [ ] **Step 2:** `grep -rn "phase 3\|install-autostart\|scheduled task\|systemd\|Send To\|not released\|checkout@v4\|artifact@v4" README.md docs/audio.md docs/files.md src/ .github/` — expected: only current text and `Superseded:` lines.
- [ ] **Step 3:** Superseded lines: phase 1 decision 8 (`a per-partner Send To entry is for phase 3's installer` → `Superseded: one "Send to other PC" entry; see src/install.rs and the phase 3 plan.`); phase 2 decisions 1 and 3 (bundle and Finder entry now come from `synergy-tools install`, `src/install.rs`).
- [ ] **Step 4:** Commit, push; watch CI by run id — green. Note the run id for the user's downloads.
- [ ] **Step 5: Hands-on check for the user** (in the hand-off message, not a file): the list below, with the run id filled in.

---

## Hands-on checks (user)

Get binaries: `gh run download <run-id>` (three artifacts).

**Linux (Openbox):**
1. Before: `systemctl --user status synergy-audio` and `ls ~/.local/bin ~/.config/autostart; grep synergy ~/.config/openbox/autostart` (note what's there).
2. `chmod +x synergy-tools && ./synergy-tools install` → prints paths; tray icon appears once. Old service gone (`systemctl --user status synergy-audio` → not found), old binaries/entries gone, exactly one autostart for synergy-tools (XDG file or one Openbox line, not both).
3. Run `./synergy-tools install` again → same output, still one tray, says one is already running.
4. Log out and in → one tray icon. Thunar right-click > Send To > "Send to other PC" on a file → arrives on the partner.
5. `~/.local/bin/synergy-tools uninstall` → tray gone, entries gone, `synergy-tools partners` (from the downloaded copy) still lists partners.

**Windows (Boot Camp):**
6. In PowerShell (not admin): `.\synergy-tools.exe install` → ends with "Run this as administrator: powershell …"; run that in an admin PowerShell. Check: Task Scheduler has no `synergy-audio`/`synergy-dragndrop`; `%LOCALAPPDATA%\synergy-audio` etc. gone; speakers not left muted; `Get-NetFirewallRule -DisplayName 'synergy-*'` shows only `synergy-tools (TCP 24851)`.
7. Tray running from `%LOCALAPPDATA%\Programs\synergy-tools`. Run `install` again while it runs → succeeds (update path). Sign out/in → one tray; audio streams to Linux.
8. Explorer: right-click a file > Send to > "Send to other PC" → arrives on Linux.
9. `& "$env:LOCALAPPDATA\Programs\synergy-tools\synergy-tools.exe" uninstall` → tray gone, Run key and Send To gone, says to delete the folder by hand.

**macOS:**
10. `xattr -d com.apple.quarantine synergy-tools; chmod +x synergy-tools; ./synergy-tools install` → `~/Applications/synergy-tools.app` exists, menu bar icon appears, "Background item added" notice.
11. Play sound → System Audio Recording prompt names **synergy-tools**; allow, Quit, start the app again (`open ~/Applications/synergy-tools.app`) → sound on Linux.
12. Log out and in → one menu bar icon, audio streams by itself.
13. Finder: right-click a file > Quick Actions > "Send to other PC" → arrives on Linux. If the entry is missing, say so: the fallback is the Automator steps in docs/files.md.
14. `~/Applications/synergy-tools.app/Contents/MacOS/synergy-tools uninstall` → icon gone, app, LaunchAgent and Quick Action gone.

**Release (after merging to main, your call):**
15. `gh workflow run release.yml --ref main` → build jobs green, `publish` skipped.
16. `git tag v0.1.0 && git push origin v0.1.0` → the Release run publishes `synergy-tools-linux-x86_64`, `synergy-tools-windows-x86_64.exe`, `synergy-tools-macos-universal` on the Releases page; the CI workflow does not run for the tag.
