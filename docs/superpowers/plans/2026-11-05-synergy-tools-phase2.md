# synergy-tools phase 2 (macOS) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The same `synergy-tools` binary builds and runs on macOS with the single tray: Audio as a sender (streams by itself to the first online Linux partner, Stop/Start), Files (tray, drop window, presence, picker, receive into Downloads, notifications), several partners including the Boot Camp case, and a `macos` CI job.

**Architecture:** No new modules and no new crates. macOS is a third `cfg` branch in the few places that are platform glue today: `notify.rs` (toast, folder opener, clipboard), `config::this_pc_name`, and `tray.rs` (no Dock icon, picker, no minimize). Audio capture on macOS is already in `src/audio.rs` (cpal 0.18: an input stream on the default output device is a CoreAudio process tap on macOS 14.6+); the sender role already follows from `audio::RECEIVER = cfg!(target_os = "linux")`. Logic that can be tested is kept in small pure functions whose tests run on Linux; macOS-only code is verified by the new CI job and by the user's hands-on checks.

**Tech Stack:** Rust 2021, tao 0.37.2 (`platform::macos::{WindowExtMacOS, EventLoopWindowTargetExtMacOS, ActivationPolicy}`), tray-icon 0.26.1 / muda 0.21.2 (`ContextMenu::show_context_menu_for_nsview`), cpal 0.18.2, macOS tools `osascript`, `open`, `pbcopy`, `scutil`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-05-unified-tray-design.md` (goal 7). Conventions and still-binding decisions: `docs/superpowers/plans/2026-11-05-synergy-tools-phase1.md`.

## Global Constraints

- Platforms: Windows, Linux and macOS. macOS is a sender only ("Changing audio direction (still Windows/macOS sender → Linux receiver)" is a non-goal).
- macOS minimum: 14.6 (CoreAudio process tap). Binary is universal (arm64 + x86_64; the Boot Camp Mac is Intel).
- Audio output device must run at 48 kHz on every sender (no resampling, unchanged).
- Ports unchanged: UDP 24850 (audio), TCP 24851 (pairing, presence, files). Config dir `dirs::config_dir()/synergy-tools/` (on macOS `~/Library/Application Support/synergy-tools/`).
- Auto-mute stays Windows-only: "Not shown on macOS, which has no auto-mute today" (spec). The tray's `Auto-mute speakers` item stays `#[cfg(windows)]`.
- No new crates. No app bundle, Info.plist, signing, installer, autostart or release workflow (phase 3).
- Anti-goals still bind: one tray icon, one pairing, CLI keeps working.
- Docs change in the same commit as the code they describe. Superseded docs get a one-line `Superseded:` note pointing at the current code, not a silent rewrite.
- Tests: each step runs one test or one filter (`cargo test <filter>`). The full suite runs once, in Task 4. No Wine/Proton.
- Every task ends with `cargo build` on Linux showing zero warnings, then a push of `phase2` and a green `CI` run (linux + windows; + macos from Task 2 on). CI runs with `RUSTFLAGS=-D warnings` from Task 2 on, so a warning on any OS fails it.
- The controller is Linux: macOS code cannot be compiled locally (coreaudio-sys needs the macOS SDK, so `cargo check --target *-apple-darwin` does not work here). It is verified only by the `macos` CI job and by the hands-on list at the end.

## Decisions made by the planner (reviewer: check these)

1. **No .app bundle this phase; run from Terminal.** LSUIElement is replaced by `ActivationPolicy::Accessory`, set in code once the tray icon exists (no Dock icon, no Cmd-Tab entry), mirroring `set_skip_taskbar(true)` on Windows/Linux. Started from Terminal, the System Audio Recording prompt is attributed to Terminal, whose bundle has the usage strings (this is how the old synergy-audio sender worked). So a bundle is not needed to run and test by hand; phase 3's installer can add one.
2. **Notifications via `osascript`** (`display notification`), no crate. The text is passed as an argv item, never spliced into the script, because file names come from the partner. They have no click action on macOS: "Open Downloads" and "Copy command" clicks are Linux/Windows only. (notify-rust's macOS backend has no click callback without a bundle either.)
3. **Finder "send" integration deferred to phase 3.** Every Finder route (Quick Action in `~/Library/Services`, "Open With" / Dock drop target, Share extension) needs either a bundle or files installed into `~/Library`, which is installer work. Until then the drop window and `synergy-tools files send` cover sending.
4. **Auto-mute stays Windows-only.** On macOS it needs unsafe CoreAudio property code (`kAudioDevicePropertyMute` on the default output device, missing on many HDMI/USB devices) plus the restore marker, none of it testable on Linux. Not trivial; the tray does not show the item on macOS.
5. **The drop window on macOS has no minimize button** (`with_minimizable(false)`): under the Accessory policy a minimized window could become hard to reach. The close button hides it to the tray, as on the other OSes.
6. **macOS PC name** = `scutil --get ComputerName` (the name shown in Sharing settings, e.g. `Zach’s MacBook Pro`), falling back to `"other PC"` as today.
7. **CI uploads an unsigned universal binary as a workflow artifact** (`synergy-tools-macos-universal`) so the user can test without building on the Mac. It is not a release.

## Review Focus

- **A partner sends a file whose name contains AppleScript** (`"` `&` `do shell script`): the macOS notification shows it as text, nothing runs. Test: Task 1 `osascript_args_keep_text_out_of_script`.
- **Boot Camp: one Mac, two OSes, one IP, paired with Linux as two partners.** Only the OS that is booted shows online; audio and files go to it; the CLI can name each twin as `<name>/<os>`. Tests: Task 3 `online_tells_bootcamp_twins_apart`, `find_twins_by_name_and_os`.
- **The Mac's name has a curly apostrophe and spaces** (`Zach’s MacBook Pro`): pairing stores it, the partners file round-trips it, the tray shows it. Test: Task 1 extends `partners_round_trip`.
- **The Mac's output device is not at 48 kHz:** the tray's audio line shows an error that says how to fix it on a Mac (Audio MIDI Setup), keeps retrying, and Files still works. Test: Task 3 `rate_error_names_each_os`.
- **System Audio Recording permission denied or not yet granted:** the tap delivers silence, not an error, so the Mac says "streaming" while Linux plays nothing. Not testable; docs (Task 3) say how to grant it, and it is in the hands-on list.

---

## Task order

| Task | Depends on | Notes |
|------|------------|-------|
| 1 Notifications and PC name on macOS | none | Linux tests only; first compiled on macOS by Task 2's CI |
| 2 Tray on macOS, `macos` CI job | 1 | the crate builds and tests on macOS from here |
| 3 Audio sender and Boot Camp | 2 | |
| 4 Full suite, docs sweep, CI, hands-on | 3 | |

Strictly sequential (each needs the previous one's macOS CI), one agent at a time.

---

### Task 1: Notifications and PC name on macOS

**Files:**
- Modify: `src/notify.rs`, `src/config.rs`, `docs/files.md`

**Interfaces:**
- Produces:
  ```rust
  // notify.rs
  #[cfg(any(target_os = "macos", test))]
  fn osascript_args(title: &str, text: &str) -> Vec<String>
  // = ["-e", "on run argv", "-e", "display notification (item 1 of argv) with title (item 2 of argv)", "-e", "end run", text, title]
  #[cfg(target_os = "macos")]
  fn toast(text: &str, _label: Option<&str>, _click: Option<OnClick>)   // spawns `osascript`, a thread waits on it; never blocks the caller
  // config.rs: this_pc_name() gains a macOS branch: `scutil --get ComputerName`, stdout trimmed
  ```
- `open_folder` uses `open` on macOS (`explorer.exe` Windows, `xdg-open` Linux). `copy` uses `pbcopy` with no args on macOS (`clip` Windows, `xclip -selection clipboard` Linux). Both stay referenced from the Linux/Windows click closures, so no dead code on macOS.
- `this_pc_name` on Linux and Windows is unchanged.

- [ ] **Step 1: Write the failing tests.** `notify::tests::osascript_args_keep_text_out_of_script`: with text `a" & (do shell script "touch /tmp/x") & "` and title `synergy-tools`, the result has length 8, the last two items are exactly the text and the title, and none of the three `-e` scripts (items 1, 3, 5) contains `do shell script`. `config::tests::partners_round_trip`: add a third partner named `Zach’s MacBook Pro` (curly apostrophe, spaces) with os `macos`; reload equals saved. `config::tests::mac_pc_name_from_scutil` under `#[cfg(target_os = "macos")]`: `this_pc_name() != "other PC"` (runs only in Task 2's CI).
- [ ] **Step 2: Run** `cargo test osascript_args` — expected FAIL to compile. `cargo test partners_round_trip` — expected PASS already (the format allows it; the test pins it).
- [ ] **Step 3: Implement** the Interfaces. `toast` on macOS: `Command::new("osascript").args(osascript_args(TITLE, text))`, spawn in a `std::thread` that calls `.status()`; an error to stderr as the other `toast`s do.
- [ ] **Step 4: Run** `cargo test osascript_args` and `cargo test partners_round_trip` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/files.md`, Pairing step 1 and "Daily use": on macOS notifications come from `osascript` (macOS may list them under Script Editor in System Settings → Notifications; allow them there if none appear) and have no click action; the pairing command is in the notification text.
- [ ] **Step 6: Commit** `git commit -m "macOS: notifications via osascript, open/pbcopy, PC name from scutil"`; push; `gh run watch --exit-status` — linux and windows green.

### Task 2: Tray on macOS, `macos` CI job

**Files:**
- Modify: `src/tray.rs`, `.github/workflows/ci.yml`, `README.md`, `docs/files.md`

**Interfaces:**
- Consumes: Task 1 (the crate's non-tray modules compile on macOS).
- Produces: `synergy-tools` (tray, CLI) compiles and its tests pass on macOS; CI job `macos`; artifact `synergy-tools-macos-universal` (one file, `synergy-tools`).
- `tray.rs` macOS glue, all under `#[cfg(target_os = "macos")]`:
  - imports `tao::platform::macos::{ActivationPolicy, EventLoopWindowTargetExtMacOS as _, WindowExtMacOS as _}`;
  - `WindowBuilder` gets `.with_minimizable(false)` (bind the builder to a variable, add the macOS line, then `build`);
  - at `StartCause::Init`, when the tray icon was built: `target.set_activation_policy_at_runtime(ActivationPolicy::Accessory)` where Linux/Windows call `window.set_skip_taskbar(true)` (that call becomes `#[cfg(any(target_os = "linux", windows))]`). Tray failed: policy stays Regular (Dock icon) and close quits, as today;
  - picker: `unsafe { menu.show_context_menu_for_nsview(window.ns_view(), None); }` beside the gtk and hwnd branches.
- Drops need no glue: tao reports `HoveredFile` / `DroppedFile` on macOS, which the existing arms handle.
- `ci.yml`: workflow-level `env: RUSTFLAGS: -D warnings`; new job:
  ```yaml
  macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup toolchain install stable --profile minimal && rustup default stable
      - run: rustup target add x86_64-apple-darwin
      - run: cargo test
      - run: cargo build --release
      - run: cargo build --release --target x86_64-apple-darwin
      - run: lipo -create -output synergy-tools target/release/synergy-tools target/x86_64-apple-darwin/release/synergy-tools
      - uses: actions/upload-artifact@v4
        with:
          name: synergy-tools-macos-universal
          path: synergy-tools
  ```

- [ ] **Step 1: Implement** the glue above. There is no Linux test for it: it is three platform calls.
- [ ] **Step 2: Run** `cargo test tray::` and `cargo build` on Linux — PASS, zero warnings (the Linux path must be unchanged).
- [ ] **Step 3: CI.** Add the workflow `env` and the `macos` job.
- [ ] **Step 4: Docs.** `README.md`: files row Platforms → `Windows, macOS, Linux (any pair)`; tray paragraph: on macOS the icon is in the menu bar, with no Dock icon; Build: macOS needs `xcode-select --install` and rustup; "CI and releases": the three jobs, warnings fail CI, the macOS universal binary is a downloadable workflow artifact (unsigned, not a release). `docs/files.md`: intro and Status say Windows, macOS and Linux; new section "macOS" (run from Terminal: `chmod +x synergy-tools && xattr -d com.apple.quarantine synergy-tools && ./synergy-tools`, or `nohup ./synergy-tools >/dev/null 2>&1 &` to close the Terminal window; menu bar icon, no Dock icon, drop window has no minimize button; if the macOS firewall asks, allow incoming connections, needed for TCP 24851; Finder integration, as a plain sentence: "A Finder send entry comes with phase 3's installer; until then drop files on the window or use `synergy-tools files send`."); "Build from source" gains the macOS line.
- [ ] **Step 5: Commit** `git commit -m "macOS: tray without Dock icon, picker, CI macos job with universal binary"`; push; `gh run watch --exit-status` — linux, windows and macos green (macos also runs Task 1's `mac_pc_name_from_scutil`). If macos fails, fix in this task.

### Task 3: Audio sender and Boot Camp

**Files:**
- Modify: `src/audio.rs`, `src/transfer.rs` (test only), `src/config.rs` (test only), `docs/audio.md`, `docs/files.md`, `README.md`

**Interfaces:**
- Consumes: Task 2's macOS build. Existing `audio::send`, `run_in_tray`, `pick_target`, `transfer::online`, `config::find` (accepts `name/os`).
- Produces: `fn rate_error(device: &str, rate: u32) -> String` in `audio.rs`, used by `config_48k` for its error. Text: `device '{device}' runs at {rate} Hz and can't do 48000 Hz. Set it to 48000 Hz (Windows: Sound settings > device Properties > Advanced; macOS: Audio MIDI Setup > the output device > Format; Linux/PipeWire: default.clock.rate = 48000) and retry.`
- No other code change: on macOS `RECEIVER` is false, so `run_in_tray` sends to `pick_target` (first online partner with os `linux`), and Stop/Start uses the existing `Ctl`; `mute` is the existing no-op `#[cfg(not(windows))]` module.

- [ ] **Step 1: Write the failing tests.** `audio::tests::rate_error_names_each_os`: `rate_error("Speakers", 44100)` contains `44100`, `Audio MIDI Setup`, `Sound settings` and `default.clock.rate`. `transfer::tests::online_tells_bootcamp_twins_apart`: set up like `online_lists_only_answering`, but both partners have the same host and port (one listener), same name `MBP`, os `macos` and `windows`, different secrets; the listener's partners file holds only the macos secret → `online` returns exactly the macos partner. `config::tests::find_twins_by_name_and_os`: partners `MBP`/macos and `MBP`/windows on the same host → `find(ps, "MBP")` errs containing `<name>/<os>`; `find(ps, "MBP/windows")` returns the windows one; `find(ps, host)` errs (two match).
- [ ] **Step 2: Run** `cargo test rate_error` — FAIL to compile. `cargo test bootcamp_twins` and `cargo test find_twins` — expected PASS already (phase 1's partner ids handle it; the tests pin it). If either fails, fix in `transfer.rs`/`config.rs` in this task.
- [ ] **Step 3: Implement** `rate_error` and use it in `config_48k`.
- [ ] **Step 4: Run** `cargo test rate_error`, `cargo test bootcamp_twins`, `cargo test find_twins` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/audio.md`: the `### macOS (sender)` Superseded line is replaced by current text: macOS 14.6+, universal binary, output device at 48 kHz (Audio MIDI Setup), run the tray from Terminal (as in docs/files.md "macOS"); it streams by itself to the first online Linux partner; Stop/Start as on Windows; no auto-mute ("mute or lower the Mac's speakers yourself"); first run asks for **System Audio Recording** permission, naming Terminal: allow it (System Settings → Privacy & Security → Screen & System Audio Recording), then Quit and start again; without it Linux gets silence while the Mac says streaming. "In the tray" section: role sentence becomes "Linux receives; Windows and macOS send". `docs/files.md` Pairing: a "Boot Camp" paragraph: pair each OS once (two partners on the Linux side, same IP is fine); only the booted one shows online; on the CLI name them `<name>/<os>` (e.g. `synergy-tools unpair MBP/windows`). `README.md`: audio row Platforms → `Windows or macOS sender → Linux receiver`; tray paragraph "Audio streams by itself from Windows or macOS to the online Linux partner".
- [ ] **Step 6: Commit** `git commit -m "macOS audio sender: 48 kHz hint, Boot Camp twins pinned by tests, docs"`; push; `gh run watch --exit-status` — all three jobs green.

### Task 4: Full suite, docs sweep, CI, hands-on

**Files:** `docs/superpowers/plans/2026-11-05-synergy-tools-phase1.md` (one Superseded line); others only if a check fails (fix in the owning file with its docs).

- [ ] **Step 1:** In the background, `cargo test > "$SCRATCH/test.log" 2>&1; cargo build --release >> "$SCRATCH/test.log" 2>&1`; read only the `test result:` lines, failures and warnings. Expected: all pass, zero warnings.
- [ ] **Step 2:** `grep -rn "phase 2\|Windows <-> Linux\|Windows ↔ Linux\|not supported" README.md docs/audio.md docs/files.md src/` — expected: no claim that macOS is unsupported or coming later. Phase 3 mentions (installer, autostart, Finder entry) stay.
- [ ] **Step 3:** In the phase 1 plan, under decision 3, add `Superseded: phase 2 brings macOS back; see docs/superpowers/plans/2026-11-05-synergy-tools-phase2.md and docs/audio.md "macOS (sender)".`
- [ ] **Step 4:** Commit, push; `gh run watch --exit-status` — linux, windows, macos green. Note the run id for the user's artifact download.
- [ ] **Step 5: Hands-on check for the user** (in the hand-off message, not a file): the list under "Hands-on checks" below, with the run id filled in.

---

## Hands-on checks (user, on the Mac; Linux tray already running)

1. `sw_vers` shows 14.6 or newer. Audio MIDI Setup: output device Format 48,000 Hz.
2. Get the binary: `gh run download <run-id> -n synergy-tools-macos-universal`, then `chmod +x synergy-tools && xattr -d com.apple.quarantine synergy-tools` (or build: `cargo build --release`).
3. `./synergy-tools` from Terminal: one icon in the menu bar, no Dock icon, the drop window at the bottom right; the menu has no "Auto-mute speakers". If the firewall asks, allow.
4. Linux tray → Pair…; on the Mac `./synergy-tools pair <host> <code>` → both status lines show the other within ~5 s; Linux `synergy-tools partners` lists the Mac with os `macos`.
5. Play sound on the Mac: macOS asks for System Audio Recording (naming Terminal); allow, Quit the tray, start it again. Sound plays on Linux; Mac audio line `Audio: streaming to <linux>`, Linux `Audio: receiving from <mac>`.
6. Stop on the Mac → both show stopped, sound ends; Start on Linux → plays again.
7. Drop two files on the Mac's drop window → both land in Linux's Downloads; a notification on each side. Drop a file on Linux's window → lands in the Mac's `~/Downloads`, notification on the Mac (if none: System Settings → Notifications → Script Editor → allow).
8. Only if a second machine can be online at the same time as the Linux box (Boot Camp's Windows can't): pair the Mac with it too and drop a file → a picker menu appears at the drop window; choose one → it arrives there. Otherwise the macOS picker stays unchecked (say so).
9. Close button hides the drop window; menu "Show drop window" brings it back.
10. Boot Camp: reboot into Windows, run `synergy-tools.exe` there and pair it with Linux too. Linux `synergy-tools partners` shows two entries (os `macos` and `windows`, same host). Only Windows shows online; audio streams from Windows. Reboot into macOS: only macOS shows online and audio streams from it.
