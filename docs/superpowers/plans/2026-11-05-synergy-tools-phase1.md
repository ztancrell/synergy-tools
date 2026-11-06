# synergy-tools phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One `synergy-tools` binary (Windows + Linux) with one tray icon, one shared partner list, audio that streams by itself, and files sent to the online partner; it replaces `synergy-audio` and `synergy-dragndrop`, whose folders are deleted.

**Architecture:** A single Rust package at the repo root (`Cargo.toml`, `src/`). The dragndrop crate becomes the base (it already has the tray, TCP listener and stream pairing); the audio crate's `main.rs` becomes `src/audio.rs`. Pairing, the partner list, presence pings and file transfer all run on TCP 24851; audio stays on UDP 24850. Both use the same per-partner secret and an 8-byte partner id derived from it.

**Tech Stack:** Rust 2021, tao 0.37 + tray-icon 0.26 (muda 0.21 `ContextMenu` for the picker), cpal 0.18, snow 0.10 (Noise NNpsk0), spake2 0.4, sha2, getrandom, dirs, gtk 0.18 / notify-rust (Linux), windows 0.62 / tauri-winrt-notification (Windows).

**Spec:** `docs/superpowers/specs/2026-11-05-unified-tray-design.md`

## Global Constraints

- Platforms this phase: Windows and Linux only. Nothing here may claim macOS support; macOS is phase 2.
- Binary name `synergy-tools` (`synergy-tools.exe`); package `synergy-tools`, version `0.1.0`, license `GPL-3.0-or-later`.
- Ports: UDP 24850 (audio), TCP 24851 (pairing, presence, files). Unchanged numbers.
- Config dir: `dirs::config_dir()/synergy-tools/`. Files: `partners` (0600 on unix), `auto-mute-off` (marker). Windows mute marker: `%LOCALAPPDATA%\synergy-tools\muted-by-us`.
- No compatibility with the old binaries' keys or wire formats; users pair once more (spec, "Migration").
- Audio role from the platform: Linux receives, every other OS sends (`cfg!(target_os = "linux")`).
- "Mute toggle" = the existing Windows auto-mute on/off, saved across restarts; never touch a mute we didn't set. Not shown on Linux.
- Stop is not saved across restarts.
- Anti-goals: no second tray icon, no launcher spawning the old programs, no second pairing, no loss of the CLI path.
- Docs change in the same commit as the code they describe. Superseded docs get a one-line `Superseded:` note pointing at the current code, not a silent rewrite.
- Tests: each step runs one test or one filter (`cargo test <filter>`). The full suite runs once, in Task 7. No Wine/Proton runs; Windows is covered by CI and the user's hands-on check.
- No dead code: `cargo build` at the end of every task shows zero warnings, except Task 3 which builds the old `synergy-audio` crate (also zero warnings there).
- Linux build deps (CI and README): `libgtk-3-dev libayatana-appindicator3-dev libasound2-dev`.

## Decisions made by the planner (reviewer: check these)

Approved by the user 2026-11-06, all eight. On #3 the user accepts the gap: during phase 1, Mac audio needs the old `synergy-audio` receiver on Linux.

1. **One package at the repo root, no shared pairing crate.** After this plan there is exactly one consumer of pairing/crypto, so a second crate is a boundary with one user; modules (`pair.rs`, `config.rs`, `proto.rs`) give the same separation. Phase 2 (macOS) is the same binary, so it does not change this.
2. **Old crates are absorbed with `git mv`, then their folders deleted** (Tasks 1 and 4), with their CI/release workflows and install scripts. Between phase 1 and phase 3 there are no install/autostart scripts and no release workflow; old GitHub releases stay as they are.
3. **The Mac loses audio until phase 2.** The new Linux tray speaks a new pairing and protocol, so the released `synergy-audio` on macOS cannot pair with it. The code does not build on macOS this phase (notifications and drop window are Windows/Linux only).
4. **Partner id = first 8 bytes of SHA-256("synergy-tools id" ‖ secret)**, identical on both sides; every connection (audio hello, files `S`, presence `H`) carries it so the listener picks the right secret. This is what makes several partners on one IP (Boot Camp) work.
5. **Pairing replaces an entry with the same (name, os)**, otherwise appends. Name and OS are exchanged during pairing and bound into the SPAKE2 confirmations.
6. **Presence = an authenticated ping** (`H` + id + Noise handshake on TCP 24851) every 5 s from the tray. Audio picks its target from it: the first online partner whose os is `linux`. A headless `audio receive` does not answer pings, so it only works with `audio send` from the CLI.
7. **Stop is a last-writer-wins register** `(stopped, epoch)` carried in every keepalive and ack; the higher epoch wins, a tie goes to "stopped". Start or Stop on either side bumps the epoch. A restarted side starts at epoch 0, so if the other side is still stopped it adopts that stop; it streams again only when both sides restarted or someone presses Start.
8. **Picker** = a context menu popped up on the drop window (muda `ContextMenu`) listing online partners. The CLI `files send` picks with `--to <name>`; with several online and no `--to` it fails and names them. Windows "Send to" (which has no terminal) gets that error as a notification; a per-partner Send To entry is for phase 3's installer.

## Review Focus

- **Old programs still autostarting hold the ports.** The tray must say which old program to stop (TCP 24851 busy: whole app refuses to start; UDP 24850 busy on Linux: tray runs, audio line shows the error). Tests: Task 1 `busy_port_message`, Task 6 `receiver_bind_busy_message`.
- **Pairing the same machine again** must not leave a stale duplicate, and Boot Camp's two OSes on one IP must stay two partners. Test: Task 2 `add_replaces_same_name_and_os`.
- **A partner's IP changed (DHCP):** it shows offline; `synergy-tools partner <name> <host>` fixes it without re-pairing. Test: Task 2 `set_host_keeps_secret`.
- **Auto-mute switched off (or Stop pressed) while we muted the speakers:** unmute at once; Quit while muted unmutes too. Test: Task 3 `mute_decision`; Quit is in Task 7's hands-on list.
- **Drop with nobody online, or picker dismissed:** a clear "no partner online" error, nothing hangs, the next drop works. Test: Task 5 `target_by_online_count`.

---

## Task order and parallelism

| Task | Depends on | Can run in parallel with |
|------|------------|--------------------------|
| 1 Root crate from synergy-dragndrop | none | none (everything builds on it; Task 3 may start alongside) |
| 2 Shared partner list and pairing | 1 | 3 |
| 3 Audio engine: stop control, auto-mute switch, stoppable send/receive | none (works in `synergy-audio/`) | 1, 2 |
| 4 Absorb audio into synergy-tools | 2, 3 | 5 |
| 5 Presence and file picker | 2 | 4 |
| 6 Tray audio section and auto-stream | 4, 5 | none |
| 7 Full suite, CI, hands-on check | 6 | none |

Within the two-agent cap: run 1 ‖ 3, then 2 (‖ 3 if still running), then 4 ‖ 5, then 6, then 7. Parallel pairs touch `src/main.rs` and the root README in different places; the second to merge rebases.

---

### Task 1: Root crate from synergy-dragndrop

**Files:**
- Move: `synergy-dragndrop/src/*` → `src/` (`gui.rs` → `src/tray.rs`), `synergy-dragndrop/{Cargo.toml,Cargo.lock,LICENSE}` → repo root
- Move: `synergy-dragndrop/README.md` → `docs/files.md`; `synergy-dragndrop/DESIGN.md` → `docs/history/synergy-dragndrop-DESIGN.md`; `synergy-dragndrop/docs/superpowers/plans/2026-11-04-synergy-dragndrop.md` → `docs/superpowers/plans/`
- Delete: `synergy-dragndrop/` (install/uninstall scripts), `.github/workflows/dragndrop-{ci,release}.yml`
- Create: `.github/workflows/ci.yml`
- Modify: `Cargo.toml`, `src/main.rs`, `src/config.rs`, `src/proto.rs`, `src/transfer.rs`, `src/notify.rs`, `src/tray.rs`, `README.md`

**Interfaces:**
- Produces: package `synergy-tools`; modules `config, tray, notify, pair, proto, store, transfer`; `config::config_dir()` = `<config_dir>/synergy-tools`; CLI: no args = tray, `pair`, `pair <host> <code>`, `peer <host>`, `unpair`, `files send <file>...`, `files listen`, `--version`. `tray::run() -> Result<(), String>` (was `gui::run`).

- [ ] **Step 1: Move with history.** `git mv` every path listed above (use `git mv` so `git log --follow` works). Rename `mod gui` to `mod tray`. In `Cargo.toml`: `name = "synergy-tools"`, `version = "0.1.0"`, comment "same choices as synergy-audio" → drop the reference.
- [ ] **Step 2: Update the expected strings in the existing tests first** (`config::tests::missing_peer_says_how_to_pair` expects `synergy-tools pair`; `transfer::tests::busy_port_message` expects `synergy-tools is probably already running` and also `or the old synergy-dragndrop`).
- [ ] **Step 3: Run** `cargo test busy_port_message` and `cargo test missing_peer` — expected FAIL on the old names.
- [ ] **Step 4: Rename user-facing strings.** `synergy-dragndrop` → `synergy-tools` in `USAGE`, `NOT_PAIRED`, `notify::TITLE`, `tray::tooltip`, `parse_code` message, the busy-port message (`"port {port} is busy: synergy-tools is probably already running, or the old synergy-dragndrop (quit it first)"`), `proto::PROLOGUE` = `b"synergy-tools files v1"`. `config_dir()` joins `"synergy-tools"`. In `main.rs` move `send`/`listen` under `["files", "send", ..]` / `["files", "listen"]` (paths from `args[2..]`).
- [ ] **Step 5: Run** both filters — PASS; `cargo build` — zero warnings.
- [ ] **Step 6: CI.** `.github/workflows/ci.yml`, name `CI`, on push to `main` and pull_request (no path filter), `permissions: contents: read`, jobs `linux` (ubuntu-22.04, apt-get the three Linux build deps from Global Constraints) and `windows` (windows-latest), each: rustup stable minimal, `cargo test`, `cargo build --release`.
- [ ] **Step 7: Docs.** `docs/files.md`: commands renamed; Install/Uninstall/autostart sections replaced by `Superseded: install and autostart scripts were removed; phase 3 adds an installer for synergy-tools. Until then run the binary by hand.` `docs/history/synergy-dragndrop-DESIGN.md` and the moved plan get a first line `Superseded: synergy-dragndrop is now part of synergy-tools; see docs/superpowers/specs/2026-11-05-unified-tray-design.md and src/.` Root `README.md`: table row for files points at `docs/files.md` and `synergy-tools`; Layout lists `src/`, `docs/`, `synergy-audio/` (until Task 4), `.github/workflows/{ci,audio-ci,audio-release}.yml`; Build section: `cargo test && cargo build --release` at the root; release table drops the dragndrop row.
- [ ] **Step 8: Commit** `git commit -m "Make synergy-dragndrop the root synergy-tools crate"`.

### Task 2: Shared partner list and pairing

**Files:**
- Modify: `src/config.rs`, `src/pair.rs`, `src/proto.rs`, `src/transfer.rs`, `src/main.rs`, `src/tray.rs`, `docs/files.md`, `README.md`

**Interfaces:**
- Consumes: Task 1 modules.
- Produces (later tasks rely on these exact names):
  ```rust
  // config.rs  (Peer, load_peer*, save_peer*, remove_peer are deleted)
  #[derive(Clone, PartialEq)]          // Debug by hand, secret redacted
  pub struct Partner { pub id: [u8; 8], pub host: String, pub secret: [u8; 32], pub os: String, pub name: String }
  pub fn load_partners_from(dir: &Path) -> Result<Vec<Partner>, String>   // missing file = Ok(vec![])
  pub fn save_partners_to(dir: &Path, ps: &[Partner]) -> Result<(), String>
  pub fn add_partner_to(dir: &Path, p: Partner) -> Result<(), String>    // replace same (name, os), else append
  pub fn load_partners() -> Result<Vec<Partner>, String>                 // + save_partners, add_partner: config_dir() wrappers
  pub fn find<'a>(ps: &'a [Partner], key: &str) -> Result<&'a Partner, String> // name (ASCII case-insensitive) or host
  pub fn resolve(target: &str, default_port: u16) -> Result<SocketAddr, String>
  // pair.rs
  pub struct Who { pub name: String, pub os: String }   // Who::this_pc() = (config::this_pc_name(), std::env::consts::OS)
  pub fn host(s: &mut (impl Read + Write), code: &str, me: &Who) -> Result<(Who, [u8; 32]), String>
  pub fn join(s: &mut (impl Read + Write), code: &str, me: &Who) -> Result<(Who, [u8; 32]), String>
  pub fn partner_id(secret: &[u8; 32]) -> [u8; 8]
  // proto.rs
  pub fn Session::client(s: S, mode: u8, id: &[u8; 8], psk: &[u8; 32]) -> Result<Self, String> // writes mode, id, then handshake
  // transfer.rs
  pub fn pair_with(host: &str, code: &str) -> Result<Partner, String>
  pub fn send_files(p: &Partner, paths: &[PathBuf]) -> Result<String, String>
  Event::Paired(String)  // the partner's name
  ```
- Partners file: one line per partner, `<id hex16> <host> <secret hex64> <os> <name>`; name is the rest of the line (may contain spaces). Written via tmp file + rename, mode 0600 on unix.
- Pairing wire (TCP, after joiner's `P`): joiner → frame `msg_a`, frame `who_j`; host → frame `msg_b ‖ confB`, frame `who_h`; joiner → frame `confA`; host → frame `OK`. `who` bytes = `os + "\n" + name`, refused if over 1000 bytes or not UTF-8. `confB = SHA256("synergy-tools confirm B" ‖ key ‖ who_j ‖ who_h)`, `confA = SHA256("synergy-tools confirm A" ‖ key ‖ who_j ‖ who_h)`, secret = `SHA256("synergy-tools secret" ‖ key)`. SPAKE2 identities `synergy-tools joiner` / `synergy-tools host`.
- Listener modes: `P` pairing (as today, saves via `add_partner_to`, host = peer IP); `S` + 8-byte id: look the id up in `load_partners_from(config_dir)`, unknown → `Event::Refused(ip)`, else `serve_one` with that secret.
- CLI: `pair`, `pair <host> <code>`, `partners` (prints `name  os  host` per line, or "Not paired."), `partner <name> <host>` (change address, keeps secret), `unpair <name>`, `files send [--to <name>] <file>...` (no `--to` and exactly one partner: that one; several: error naming them and `--to`). `peer` is removed.
- Tray (until Task 5): status line `Paired with <names>` / `Not paired`; a drop goes to the only partner, or fails with the same "several partners" error.

- [ ] **Step 1: Write the failing tests** in `config.rs`: `partners_round_trip` (two partners, one name with a space, reload equals saved), `add_replaces_same_name_and_os` (same name+os → 1 entry with the new host/secret; same name, other os → 2), `missing_partners_file_is_empty`, `damaged_partners_file_rejected` (error contains `pair again`), `partners_file_is_private` (unix, mode 0o600), `partner_debug_hides_secret`, `set_host_keeps_secret` (find + change host + save + reload: same secret and id, new host), `find_by_name_or_host` (`"PC-A"` matches name `pc-a`; unknown key error lists names), `resolve_default_port` (`resolve("127.0.0.1", 24851)` port 24851). In `pair.rs`: `same_code_same_secret_and_names` (each side returns the other's `Who`), `wrong_code_fails_both_sides` (kept), `oversized_who_refused`. In `transfer.rs`: update `sends_two_files`, `wrong_secret_refused`, `pairing_only_when_enabled` (partners file has one entry with the joiner's name) to `Partner`; add `second_pairing_adds_partner` (two pairings with different `Who` → 2 entries) and `unknown_partner_id_refused` (send with an id not in the file → `Refused`).
- [ ] **Step 2: Run** `cargo test config::` , `cargo test pair::`, `cargo test transfer::` — expected FAIL to compile (missing `Partner`, `Who`).
- [ ] **Step 3: Implement** the Interfaces above. `partner_id` = first 8 bytes of `SHA256("synergy-tools id" ‖ secret)`. Update `tray.rs` and `main.rs` callers; `files send` and the tray drop use a shared `fn only_partner(ps: Vec<Partner>, to: Option<&str>) -> Result<Partner, String>` in `transfer.rs`.
- [ ] **Step 4: Run** the three filters — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/files.md`: Pairing section (pair adds a partner, `partners`, `partner`, `unpair <name>`, `--to`), "How it works"/"Security notes" (partner id, names bound into the confirmation). `README.md`: one paragraph "Pairing" (one code pairs audio and files; pairing again adds a partner).
- [ ] **Step 6: Commit** `git commit -m "Shared partner list: pairing again adds a partner"`.

### Task 3: Audio engine: stop control, auto-mute switch, stoppable send/receive

Works inside the old crate (`synergy-audio/`), which still builds and has its own CI, so it can run in parallel with Tasks 1–2. Its CLI keeps its current behaviour.

**Files:**
- Modify: `synergy-audio/src/main.rs`, `synergy-audio/README.md` (sections "How it works", "Pairing and encryption")

**Interfaces:**
- Produces (Task 4 moves these unchanged into `src/audio.rs`; Task 6 uses them):
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Default)]
  pub struct Ctl { pub stopped: bool, pub epoch: u64 }
  impl Ctl {
      pub fn encode(self) -> [u8; 9];               // [stopped as u8][epoch u64 LE]
      pub fn decode(b: &[u8]) -> Option<Ctl>;       // exactly 9 bytes, first byte 0 or 1
      pub fn merge(&mut self, remote: Ctl) -> bool; // adopt if remote.epoch > epoch, or equal epoch and remote.stopped && !stopped; true if changed
  }
  #[derive(Clone, Debug, PartialEq)]
  pub enum Status { Waiting, Streaming([u8; 8]), Stopped, Error(String) }   // Streaming carries the partner id
  pub struct Shared { pub ctl: Mutex<Ctl>, pub auto_mute: AtomicBool, pub status: Mutex<Status> }
  impl Shared { pub fn new(auto_mute: bool) -> Arc<Shared>; pub fn set_stopped(&self, on: bool) } // set_stopped: epoch += 1
  pub fn send(addr: SocketAddr, peer: [u8; 8], psk: [u8; 32], sh: Arc<Shared>, quit: Arc<AtomicBool>) -> Result<(), String>
  pub fn receive(sock: UdpSocket, buffer_ms: usize, load_peers: fn() -> Result<HashMap<[u8; 8], [u8; 32]>, String>, sh: Arc<Shared>, quit: Arc<AtomicBool>) -> Result<(), String>
  fn mute_action(want: bool, muted: bool, muted_by_us: bool) -> MuteAction
  mute::update(want: bool, by_us: &mut bool) -> String
  Rx::handle(&mut self, buf: &[u8], ctl: &mut Ctl) -> In
  ```
- Wire change: keepalive payload (sender → receiver) and ack payload (receiver → sender) are `Ctl::encode()`, 9 bytes; any other length is `Rejected`. Each side merges the remote `Ctl` into its own and answers with its own.
- Gating: sender drops captured audio while `ctl.stopped`; receiver discards audio and clears the jitter buffer while stopped. Mute: `want = ack fresh && !ctl.stopped && auto_mute`.
- Status: sender `Streaming(peer)` while ack fresh and not stopped; receiver `Streaming(peer of cur session)` while an authentic audio packet arrived within `ACK_FRESH`; `Stopped` while stopped; else `Waiting`.
- `send` returns `Ok(())` within 1 s of `quit` being set, after dropping the stream, ending its threads and unmuting if it muted. `receive` takes an already bound socket (Task 6 binds it to report a busy port) and returns within 1 s of `quit`. `Rx` calls `load_peers` again when a hello carries an unknown id, at most once per second.

- [ ] **Step 1: Write the failing tests:** `ctl_encode_decode` (round trip; 8 or 10 bytes → None; first byte 2 → None), `ctl_merge_newer_wins`, `ctl_tie_stop_wins`, `stop_from_sender_reaches_receiver` (Rx handles a keepalive sealed with `Ctl{stopped:true,epoch:1}` → its ctl is stopped and the ack decodes to the same), `stopped_receiver_drops_audio` (receiver ctl stopped: an audio packet is not returned as `In::Audio`), `unknown_id_reloads_peers_once_per_second` (counter in a test `load_peers`: two unknown hellos within 1 s → one reload), `keepalive_stops_on_quit` (keepalive on a loopback socket with no receiver; set quit; thread joins within 2 s). Update `session_round_trip_tamper_replay_unpaired` (keepalives/acks carry a `Ctl`) and `mute_decision` (`want=false, muted, by_us` → `Unmute`; `want=true, muted, !by_us` → `Nothing`; `want=false, !muted` → `Nothing`).
- [ ] **Step 2: Run** `cargo test ctl_` (in `synergy-audio/`) — expected FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces. The keepalive loop checks `quit` each tick; `send`'s stats loop sleeps 100 ms steps instead of 5 s. The CLI `send`/`receive` paths build `Shared::new(true)` and a never-set `quit`; `receive` binds UDP `PORT` itself before calling the engine and passes `load_senders` as `load_peers`, so behaviour is unchanged.
- [ ] **Step 4: Run** `cargo test ctl_`, `cargo test stop`, `cargo test mute_decision`, `cargo test keepalive_stops_on_quit`, `cargo test unknown_id` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** Module doc comment and README "How it works": keepalive and ack carry the 9-byte stop control (LWW by epoch, tie = stopped).
- [ ] **Step 6: Commit** `git commit -m "audio: stop control message, auto-mute switch, stoppable send/receive"`.

### Task 4: Absorb audio into synergy-tools

**Files:**
- Move: `synergy-audio/src/main.rs` → `src/audio.rs`; `synergy-audio/README.md` → `docs/audio.md`
- Delete: rest of `synergy-audio/` (Cargo files, LICENSE, `dev.sh`, `update.ps1`, install/uninstall scripts), `.github/workflows/audio-{ci,release}.yml`
- Modify: `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/config.rs` (only if a helper is missing), `README.md`

**Interfaces:**
- Consumes: Task 2 `Partner`, `load_partners`, `find`, `resolve(target, port)`, `hex`; Task 3 engine.
- Produces: `audio::PORT = 24850`, `audio::DEFAULT_BUFFER_MS`, `pub fn audio::peers() -> Result<HashMap<[u8; 8], [u8; 32]>, String>` (id → secret from the partner list; empty → `not paired yet: run \`synergy-tools pair\` on one machine, then \`synergy-tools pair <host> <code>\` on the other`), `pub fn audio::bind_receiver(port: u16) -> Result<UdpSocket, String>`, `pub(crate) mod mute { pub fn restore() }`.
- CLI: `audio send <host[:port]> [--log <file>]` (partner = `find(partners, host part)`), `audio receive [--buffer-ms N]`, `audio unmute`. `--log` keeps its file rules; `detach_console` is deleted (the binary is GUI-subsystem with `attach_console` already).

- [ ] **Step 1: Move** with `git mv`; add `mod audio;`; Cargo.toml gains `cpal = "0.18"` and the Windows `windows = { version = "0.62", features = [...] }` exactly as in the old crate. Delete from `audio.rs`: `main`, UDP pairing (`PAIR1..4`, `pair_receiver`, `exchange`, `pair_sender`, `spake`, `pair_receive`, `pair_send`, `unpair`, `random`, `digest`), `config_dir`, `write_secret`, `hex`, `unhex`, `load_receiver`, `load_senders`, `NOT_PAIRED`, `detach_console`, and the pairing tests. Mute marker → `synergy-tools`.
- [ ] **Step 2: Write the failing test** `audio::tests::peers_come_from_partner_list` (two partners in a temp dir via a `peers_from(dir: &Path)` the public `peers()` wraps → map with both ids and secrets) and update `config::tests::resolve_default_port` to also check 24850.
- [ ] **Step 3: Run** `cargo test peers_come_from` — FAIL.
- [ ] **Step 4: Implement** `peers`, `bind_receiver` (error text: `can't bind UDP {port}: {e}. Is another synergy-tools or the old synergy-audio receiver running? Stop it (old one: systemctl --user disable --now synergy-audio)`), the `audio` CLI arms in `main.rs`, `USAGE` lines.
- [ ] **Step 5: Run** `cargo test audio::` and `cargo test resolve_default_port` — PASS; `cargo build` — zero warnings; `cargo tree -d -e normal -i windows@0.62` shows one copy.
- [ ] **Step 6: Docs.** `docs/audio.md`: commands become `synergy-tools audio …`, pairing section points at `synergy-tools pair` (one pairing for both tools); sections "Download and install", autostart (Linux and Windows), "Dev scripts", "Releases and CI", "Making a release" each replaced by one `Superseded:` line (installers/autostart/releases come in phase 3; the tray starts audio by itself, Task 6); the macOS sender section gets `Superseded: macOS is not supported by synergy-tools until phase 2; the released synergy-audio cannot pair with it.` Root `README.md`: table row → `docs/audio.md`; Layout drops `synergy-audio/` and the audio workflows; release table removed (`Superseded: per-tool releases end here; phase 3 adds one release workflow.`); Build deps line uses the three Linux packages.
- [ ] **Step 7: Commit** `git commit -m "Absorb synergy-audio into synergy-tools (audio send/receive/unmute)"`.

### Task 5: Presence and file picker

**Files:**
- Modify: `src/proto.rs` (mode `H`), `src/transfer.rs`, `src/tray.rs`, `src/main.rs`, `docs/files.md`, `README.md`

**Interfaces:**
- Consumes: Task 2 `Partner`, `Session::client(s, mode, id, psk)`.
- Produces:
  ```rust
  // transfer.rs
  pub fn ping(p: &Partner) -> bool                       // connect (1 s timeout) + 'H' + id + Noise handshake; true only if it completes
  pub fn online(ps: &[Partner]) -> Vec<Partner>          // pings in parallel (std::thread::scope), keeps list order
  pub fn watch_online(online: Arc<Mutex<Vec<Partner>>>, changed: impl Fn() + Send + 'static) -> !  // every PRESENCE_EVERY = 5 s; calls changed() when the set of ids changes
  pub enum Target { Nobody, One(Partner), Several(Vec<Partner>) }
  pub fn target(online: Vec<Partner>, to: Option<&str>) -> Result<Target, String> // `to` filters by name via config::find
  // tray.rs
  UserEvent::Online                                      // wake: re-read the shared online list
  ```
- Listener mode `H` + id: unknown id → close silently (no event); known → `Session::server`, then close. No event either way.
- Tray: status line `Not paired` / `No partner online` / `Online: A, B`. Drop: `Nobody` → error notification `not sent: no partner online`; `One` → send; `Several` → a `Menu` with one `MenuItem` per name, shown by `ContextMenu::show_context_menu_for_gtk_window` (Linux, `window.gtk_window()` upcast to `gtk::Window`) or `show_context_menu_for_hwnd` (Windows, `window.hwnd()`); the chosen `MenuId` (arriving as `UserEvent::Menu`) sends the held files. A new drop replaces held files (`ponytail:` note: dismissing the picker leaves nothing to clean up). The 10 s `PEER_CHECK` reload is replaced by `watch_online`.
- CLI `files send [--to <name>] <file>...` uses `online(load_partners)` + `target`: `Nobody` → `not sent: no partner online`; `Several` → `several partners online (A, B): use --to <name>`. `only_partner` from Task 2 is deleted.

- [ ] **Step 1: Write the failing tests** in `transfer.rs`: `ping_known_partner_true`, `ping_unknown_id_false`, `ping_nobody_listening_false_fast` (< 2 s), `online_lists_only_answering` (two partners, one listener), `target_by_online_count` (0 → `Nobody`, 1 → `One`, 2 → `Several`, 2 with `to` = one name → `One`, `to` unknown → Err listing names).
- [ ] **Step 2: Run** `cargo test ping_` — FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `cargo test ping_`, `cargo test online_lists`, `cargo test target_by` — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/files.md`: "Daily use" (drop goes to the online partner; picker when several), "Command line" (`--to`), "How it works" (`H` presence ping every 5 s). `README.md`: tray status line.
- [ ] **Step 6: Commit** `git commit -m "Files go to the online partner, with a picker when several are online"`.

### Task 6: Tray audio section and auto-stream

**Files:**
- Modify: `src/config.rs`, `src/audio.rs`, `src/tray.rs`, `docs/audio.md`, `README.md`

**Interfaces:**
- Consumes: Task 3 `Shared`, `Status`, `send`, `receive`; Task 4 `peers`, `bind_receiver`, `mute::restore`; Task 5 shared `online` list and `UserEvent::Online`.
- Produces:
  ```rust
  // config.rs
  pub fn auto_mute_on_in(dir: &Path) -> bool              // true unless `auto-mute-off` exists; + auto_mute_on() wrapper
  pub fn set_auto_mute_in(dir: &Path, on: bool) -> Result<(), String>  // + set_auto_mute(on)
  // audio.rs
  pub const RECEIVER: bool = cfg!(target_os = "linux");
  pub fn pick_target(online: &[Partner]) -> Option<&Partner>   // first with os == "linux"
  pub fn run_in_tray(sh: Arc<Shared>, online: Arc<Mutex<Vec<Partner>>>)  // spawns and returns
  // tray.rs
  fn audio_line(st: &Status, name_of: impl Fn(&[u8; 8]) -> String) -> String
  ```
- `run_in_tray`, receiver: `bind_receiver(PORT)` once; on error set `Status::Error(msg)` and stop (tray keeps running); else `receive(sock, DEFAULT_BUFFER_MS, peers, sh, never-set quit)`, on device error set `Status::Error` and retry every 10 s. Sender: every 1 s compare `pick_target(online)` with the running target's id; on change set the running `quit`, join, start `send` for the new one on its own thread; none → `Status::Waiting`; `send` error → `Status::Error`, retry after 10 s.
- Menu, top to bottom: status line; separator; audio line (disabled); `Stop audio` / `Start audio` (one item, text follows `ctl.stopped`, calls `sh.set_stopped(!stopped)`); Windows only: `CheckMenuItem` `Auto-mute speakers` (initial `auto_mute_on()`, toggling sets `sh.auto_mute` and `set_auto_mute`); separator; `Hide drop window` / `Show drop window`; separator; `Pair…`; `Quit`. Quit calls `audio::mute::restore()` before exiting.
- `audio_line` texts: receiver `Audio: receiving from <name>` / sender `Audio: streaming to <name>`; `Audio: stopped`; `Audio: waiting for partner`; `Audio: error: <msg>` truncated to 60 chars. The tray wakes every 1 s (`AUDIO_POLL`) to refresh it.

- [ ] **Step 1: Write the failing tests:** `config::tests::auto_mute_default_on_and_saved` (fresh dir → on; set off → off after re-read; set on → marker gone), `audio::tests::target_is_first_online_linux_partner` (windows, linux A, linux B → A; no linux → None), `audio::tests::receiver_bind_busy_message` (bind a UDP socket on an ephemeral port, `bind_receiver` on it → Err containing `synergy-audio`), `tray::tests::audio_line_texts` (each `Status`, error truncation).
- [ ] **Step 2: Run** `cargo test auto_mute_default`, `cargo test target_is_first`, `cargo test receiver_bind_busy`, `cargo test audio_line` — FAIL.
- [ ] **Step 3: Implement** the Interfaces; `tray::start` creates `Shared::new(config::auto_mute_on())` and calls `audio::run_in_tray` after the listener binds.
- [ ] **Step 4: Run** the four filters — PASS; `cargo build` — zero warnings.
- [ ] **Step 5: Docs.** `docs/audio.md`: new top section "In the tray" (streams by itself when both sides run and are paired; role by platform; Stop/Start on either side, not saved; Auto-mute toggle on Windows, saved; headless `audio receive` must not run with the tray and does not answer presence pings). `README.md`: the full tray menu.
- [ ] **Step 6: Commit** `git commit -m "Tray: Audio section, auto-stream, Stop/Start, auto-mute toggle"`.

### Task 7: Full suite, CI and hands-on check

**Files:** none unless a check fails (then fix in the owning file and commit with its docs).

- [ ] **Step 1:** In the background, `cargo test > "$SCRATCH/test.log" 2>&1; cargo build --release >> "$SCRATCH/test.log" 2>&1`; read only the `test result:` lines, failures and warnings. Expected: all pass, zero warnings.
- [ ] **Step 2:** `grep -rn "synergy-dragndrop\|synergy-audio" --include='*.rs' --include='*.md' --include='*.yml' .` outside `docs/history/`, `docs/superpowers/` and `Superseded:` lines: expected only intentional mentions of the old programs (busy-port messages, migration notes).
- [ ] **Step 3:** Push; `gh run watch` the `CI` run: linux and windows green.
- [ ] **Step 4: Hands-on check for the user** (written into the hand-off message, not a file). First stop the old programs on both machines (Linux: `systemctl --user disable --now synergy-audio`, remove any `synergy-dragndrop` desktop autostart entry, quit its tray; Windows: `Unregister-ScheduledTask synergy-audio`, `Unregister-ScheduledTask synergy-dragndrop`, quit the old trays). Then: run `synergy-tools` on both; Linux tray → Pair…, run the shown command on Windows; within ~5 s both status lines show the other as online and audio plays on Linux with Windows speakers muted; Stop on Linux → Windows unmutes and both show "stopped"; Start on Windows → plays again; untick Auto-mute → Windows unmutes at once; restart the Windows tray → the tick is still off; drop a file on each drop window → arrives in the other's Downloads; Quit while streaming on Windows → speakers unmuted.

---

Later phases (not in this plan): phase 2 = macOS tray and Files port; phase 3 = installers, autostart, removal of old autostart entries, migration, one release workflow.
