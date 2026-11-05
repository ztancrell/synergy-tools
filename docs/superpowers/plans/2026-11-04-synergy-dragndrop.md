# synergy-dragndrop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One Rust program, run on both a Windows 10/11 PC and a Linux (KDE Plasma Wayland) PC, that sends files to the paired other PC from a drop window or a right-click "Send to other PC" entry and saves received files to Downloads, safely.

**Architecture:** A pure, unit-tested core (wire protocol over any `Read + Write`, SPAKE2 pairing, Noise-encrypted sessions, receive-to-folder with temp-name-then-link, config) with a headless CLI (`pair`, `send`, `listen`) on top. The GUI process (`synergy-dragndrop` with no arguments: tao drop window + tray-icon tray + listener thread) and OS integration (notifications, Mark of the Web, install scripts with Send To / Dolphin / Nautilus entries and firewall) sit outside the core. "Send to" runs a short-lived `synergy-dragndrop send <files…>` that connects to the peer directly (no IPC to the running instance).

**Tech Stack:** Rust 2021, stable. Crates: `snow 0.10` (Noise), `spake2 0.4` (pairing), `sha2 0.10`, `getrandom 0.3` (same set and versions as synergy-audio; newer sha2 0.11 / getrandom 0.4 exist but would duplicate what snow and spake2 pull in), `dirs 7` (Downloads + config dirs), `tao 0.37` (drop window, GTK3 on Linux so file drop works on Wayland), `tray-icon 0.26` (feature `ksni` on Linux: pure D-Bus StatusNotifierItem, no libappindicator), `notify-rust 4` (Linux notifications), `tauri-winrt-notification 0.8` (Windows toasts with click callback). Nothing else.

**Spec:** `/home/zpt/projects/synergy-dragndrop/DESIGN.md` (source of truth, including "Decided"). Sibling for conventions: `/home/zpt/projects/synergy-audio` (`src/main.rs`, `install-autostart.sh/.ps1`, `.github/workflows/ci.yml` + `release.yml`, `README.md`).

## Global Constraints

- Rust; one program with the same role on both sides (each machine sends and receives).
- TCP port **24851** (synergy-audio uses UDP 24850). LAN only; peer given as IP or hostname, optional `:port`.
- Received files go to the user's Downloads folder (`dirs::download_dir()`).
- Collision names: `name (1).ext`, `name (2).ext`, …; **never overwrite**.
- Partial files are written under a temporary name and appear under their real name only after the full file is received and checked.
- Folders are refused before anything is sent, with the message: `not sent: <name> is a folder, zip it first`.
- Unreachable peer: fail at once (3 s connect timeout), notification `not sent: other PC unreachable`. No queue, no retry.
- Only the paired partner may send; any other connection is refused. Pairing and transfers are authenticated and encrypted.
- Windows: received files get a `Zone.Identifier` stream with `ZoneId=3`; no warning dialog of our own. Linux: received files are created without the execute bit (mode 0644 before umask).
- Notification on receive: `N files received from <other PC>` (`1 file received from …` for one); clicking it opens the Downloads folder.
- Tray icon on both sides, status only (not a drop target); menu: Show/hide drop window, Pair…, Quit.
- Windows setup asks for admin once (one UAC prompt) to add an inbound firewall rule for TCP 24851, Profiles Private **and** Public. Linux setup opens TCP 24851 in ufw/firewalld like synergy-audio's script.
- Starts at login on both machines.
- Windows 10/11 x86_64 and Linux x86_64 prebuilt releases, CI and release workflow in synergy-audio's style (ubuntu-22.04, windows-latest, `+crt-static`, version guard, SHA256SUMS).
- Error style as in synergy-audio: `Result<T, String>` with user-readable messages; no error-type hierarchy.
- Docs change in the same commit as the code they describe (README, DESIGN.md, script header comments). Every task's commit includes its README update.
- Do **not** run Wine/Proton. Windows-only code is verified with `cargo check --target x86_64-pc-windows-gnu` locally (`rustup target add x86_64-pc-windows-gnu` once) and CI on windows-latest, plus the user's hands-on check.
- Narrowest test run per step (`cargo test <filter>`); full `cargo test` once at the end of each task.

## Review Focus

1. **Hostile or odd file names from the peer** (`../x`, `C:\x`, `a/b`, `CON`, `aux.txt`, trailing dot/space, `:` in a Linux name, empty, 300-byte names): must be saved as a plain, Windows-legal name inside Downloads, never outside it. Test: `store::tests::sanitize_*` (Task 2).
2. **Connection dropped mid-file** (peer closes, Wi-Fi drops, sender killed): no file under the real name, temp file deleted. Test: `transfer::tests::truncated_stream_leaves_nothing` (Task 5).
3. **Two files with the same name in one batch, or the name already taken between check and rename**: both kept, `x.zip` and `x (1).zip`, nothing overwritten (link-based rename fails on existence instead of replacing). Test: `store::tests::same_name_twice_in_batch` (Task 2) and `store::tests::finish_never_replaces_existing` (Task 2).
4. **Disk full / Downloads not writable on the receiver**: sender gets `not sent: <reason from other PC>` instead of a false "sent", temp removed. Test: `transfer::tests::receiver_error_reaches_sender` (Task 5).
5. **The GUI already running when the user runs `pair` or `listen` in a terminal** (port 24851 busy): a clear message pointing to the tray's "Pair…" or to quitting the tray app, not a raw bind error. Test: `transfer::tests::busy_port_message` (Task 5).

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | deps, `[target.'cfg(…)'.dependencies]` split, `strip = true` |
| `src/main.rs` | CLI dispatch + usage text, `windows_subsystem`/console attach, logging helper |
| `src/config.rs` | config dir, peer file (`<host> <secret-hex>`, 0600), hex helpers, `this_pc_name()` |
| `src/store.rs` | receive-to-folder: name sanitizing, collision naming, temp file, finish (no-overwrite), Mark of the Web, file mode |
| `src/proto.rs` | framing, Noise session wrapper, message encode/decode (generic over `Read + Write`) |
| `src/pair.rs` | SPAKE2 pairing over a stream (host side and join side) |
| `src/transfer.rs` | `send_files` (client), `Listener` (server loop, pairing mode), TCP timeouts |
| `src/notify.rs` | desktop notifications + "open Downloads" (Linux: notify-rust; Windows: tauri-winrt-notification) |
| `src/gui.rs` | tao drop window, tray icon + menu, wiring to `transfer` and `notify` |
| `install-autostart.sh`, `uninstall-autostart.sh` | Linux install: binary, XDG autostart, Dolphin + Nautilus entries, firewall |
| `install-autostart.ps1`, `uninstall-autostart.ps1` | Windows install: binary, scheduled task, Send To shortcut, firewall (UAC once) |
| `.github/workflows/ci.yml`, `release.yml` | CI and tagged releases, copied from synergy-audio and adapted |
| `README.md`, `LICENSE` | user docs in synergy-audio's style; GPL-3.0-or-later like the sibling (copy its LICENSE) |

Tests live in `#[cfg(test)] mod tests` at the bottom of each core file (synergy-audio style). Core files (`config`, `store`, `proto`, `pair`, `transfer`) must not import `tao`, `tray-icon` or notification crates.

## Wire protocol (decided here, used by Tasks 3-5)

- **Frame:** `u16` big-endian length + bytes (max 65535). Every message below is one frame.
- **First byte of a connection** (before any frame): `b'P'` = pairing, `b'S'` = session.
- **Pairing (plain frames):** joiner → `SPAKE2 msg A`; host → `msg B ‖ sha256("synergy-dragndrop confirm B" ‖ key)`; joiner → `sha256("synergy-dragndrop confirm A" ‖ key)`; host → `b"OK"` (or closes on mismatch). Secret = `sha256("synergy-dragndrop secret" ‖ key)`. SPAKE2 identities `b"synergy-dragndrop joiner"` (A) and `b"synergy-dragndrop host"` (B). Code: 8 random digits shown `1234-5678`. The host allows **one** attempt, then leaves pairing mode; timeout 120 s.
- **Session:** `Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`, PSK = secret, prologue `b"synergy-dragndrop v1"`. Client sends handshake msg 1, server replies msg 2. A failed handshake = not the paired PC: server logs `refused connection from <ip>: not the paired PC` and closes.
- **Transport messages** (Noise plaintext, each ≤ 65535 − 16):
  - `b'B' ‖ u16 count ‖ sender-name utf8` — batch header (client)
  - `b'F' ‖ u64 size ‖ name utf8` — file header (client)
  - `b'D' ‖ bytes` — data chunk, ≤ 60 000 bytes (client)
  - `b'E'` — end of file; server checks bytes received == size, then finishes the file
  - `b'K' ‖ u16 saved` — batch done (server), or `b'X' ‖ reason utf8` — failure (server), then close
- Stream read timeout 30 s on both sides (stall = error). Connect timeout 3 s.

---

### Task 1: Scaffold, config, CI

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/config.rs`, `.github/workflows/ci.yml`, `README.md` (title, CI badge, one-paragraph idea, "Status: in development", build-from-source), `LICENSE` (copy from synergy-audio)
- Test: `src/config.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `pub const PORT: u16 = 24851;` (in `main.rs`, `pub(crate)`)
  - `pub struct Peer { pub host: String, pub secret: [u8; 32] }`
  - `pub fn config_dir() -> Result<PathBuf, String>` — `dirs::config_dir()/synergy-dragndrop`
  - `pub fn load_peer_from(dir: &Path) -> Result<Peer, String>` / `pub fn save_peer_to(dir: &Path, p: &Peer) -> Result<(), String>` (file `peer`, text `"<host> <64-hex>\n"`, 0600 on unix, written via temp + rename); `load_peer()` / `save_peer()` wrap them with `config_dir()`
  - `pub fn remove_peer() -> Result<bool, String>`
  - `pub fn hex(b: &[u8]) -> String`, `pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]>` (copy from synergy-audio)
  - `pub fn this_pc_name() -> String` — `COMPUTERNAME` env on Windows, `/proc/sys/kernel/hostname` trimmed on Linux, fallback `"other PC"`
  - `pub fn resolve(target: &str) -> Result<SocketAddr, String>` — synergy-audio's `resolve` with `PORT`

- [ ] **Step 1: Write failing tests** in `config.rs`:

```rust
#[test] fn peer_round_trip() { /* tempdir under std::env::temp_dir()/unique */
    save_peer_to(&d, &Peer { host: "192.168.1.5".into(), secret: [7; 32] }).unwrap();
    let p = load_peer_from(&d).unwrap();
    assert_eq!((p.host.as_str(), p.secret), ("192.168.1.5", [7; 32])); }
#[test] fn missing_peer_says_how_to_pair() {
    assert!(load_peer_from(&empty).unwrap_err().contains("synergy-dragndrop pair")); }
#[test] fn damaged_peer_file_rejected() { /* write "host zz" */ assert!(load_peer_from(&d).unwrap_err().contains("damaged")); }
#[cfg(unix)] #[test] fn peer_file_is_private() { /* mode & 0o777 == 0o600 */ }
#[test] fn resolve_default_port() { assert_eq!(resolve("127.0.0.1").unwrap().port(), 24851); }
```

- [ ] **Step 2:** `cargo test config` → FAIL (not defined).
- [ ] **Step 3:** Implement the interfaces above. `main.rs`: `mod config;` etc., `--version`, and a usage text listing every subcommand of the final CLI (`pair`, `pair <host> <code>`, `peer <host>`, `unpair`, `send <file>…`, `listen`, no args = tray app, `--version`); unimplemented ones return `Err("not implemented yet")` until their task. `Cargo.toml`: name `synergy-dragndrop`, version `0.1.0`, `license = "GPL-3.0-or-later"`, core deps only (`snow` with `default-features = false, features = ["default-resolver-crypto"]`, `spake2 = "0.4"`, `sha2 = "0.10"`, `getrandom = "0.3"`, `dirs = "7"`), `[profile.release] strip = true`. No tempdir crate: tests make a unique dir under `std::env::temp_dir()` with `getrandom`.
- [ ] **Step 4:** `cargo test config` → PASS; `cargo check --target x86_64-pc-windows-gnu` → OK.
- [ ] **Step 5:** `ci.yml`: synergy-audio's `linux` and `windows` jobs (no macOS, no libasound); Linux job adds `sudo apt-get install -y libgtk-3-dev` once Task 7 needs it (note it there).
- [ ] **Step 6: Commit** `feat: scaffold, config and peer file` (with README).

### Task 2: Receive-to-folder (`store.rs`)

**Files:**
- Create: `src/store.rs` (+ tests)

**Interfaces:**
- Produces:
  - `pub fn sanitize_name(raw: &str) -> Option<String>` — last path component after splitting on `/` and `\`; replace `<>:"|?*` and control chars with `_`; strip trailing `.`/space; prefix `_` to Windows reserved stems (`CON PRN AUX NUL COM1-9 LPT1-9`, case-insensitive, with or without extension); truncate to 200 bytes on a char boundary keeping the extension; `None` for empty, `.` or `..`.
  - `pub fn candidate(name: &str, n: u32) -> String` — `n == 0` → `name`; else `stem (n).ext` using the **last** extension (`a.tar.gz` → `a.tar (1).gz`; `.bashrc` → `.bashrc (1)`; `README` → `README (1)`).
  - `pub struct Incoming { file: File, tmp: PathBuf, dir: PathBuf, name: String, written: u64 }`
  - `pub fn begin(dir: &Path, name: &str) -> Result<Incoming, String>` — creates `dir/.<name>.<8 hex>.part` with `create_new`, mode 0o644 on unix; on Windows writes `<tmp>:Zone.Identifier` = `"[ZoneTransfer]\r\nZoneId=3\r\n"` (plain `std::fs::write` on the ADS path; the stream follows the file through the hard link below).
  - `impl Incoming { pub fn write(&mut self, b: &[u8]) -> Result<(), String>; pub fn finish(self, expected: u64) -> Result<PathBuf, String>; pub fn abort(self) }`
  - `finish`: error (and remove temp) if `written != expected`; `sync_all`; then for `n in 0..` try `std::fs::hard_link(tmp, dir/candidate(name, n))` — `AlreadyExists` → next `n`; success → remove tmp, return final path. If `hard_link` is unsupported (FAT/exFAT), fall back to `if !exists { rename }` (ponytail comment: small race on non-NTFS/ext filesystems). `abort` removes the temp. `Drop` on an unfinished `Incoming` also removes the temp.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn sanitize_strips_paths() { assert_eq!(sanitize_name("../../etc/x").as_deref(), Some("x"));
    assert_eq!(sanitize_name(r"C:\Users\a\b.exe").as_deref(), Some("b.exe")); }
#[test] fn sanitize_windows_illegal() { assert_eq!(sanitize_name("a:b?.txt").as_deref(), Some("a_b_.txt"));
    assert_eq!(sanitize_name("CON").as_deref(), Some("_CON")); assert_eq!(sanitize_name("aux.txt").as_deref(), Some("_aux.txt"));
    assert_eq!(sanitize_name("x. ").as_deref(), Some("x")); }
#[test] fn sanitize_rejects_empty() { for s in ["", ".", "..", "a/"] { assert_eq!(sanitize_name(s), None) } }
#[test] fn sanitize_long_name_keeps_ext() { let n = sanitize_name(&("é".repeat(300) + ".zip")).unwrap();
    assert!(n.len() <= 200 && n.ends_with(".zip")); }
#[test] fn candidate_names() { assert_eq!(candidate("a.zip", 0), "a.zip"); assert_eq!(candidate("a.zip", 2), "a (2).zip");
    assert_eq!(candidate("a.tar.gz", 1), "a.tar (1).gz"); assert_eq!(candidate(".bashrc", 1), ".bashrc (1)");
    assert_eq!(candidate("README", 1), "README (1)"); }
#[test] fn temp_not_visible_until_finished() { /* begin+write: dir has no "x.zip", has one ".x.zip.*.part" */ }
#[test] fn finish_checks_size() { /* write 3 bytes, finish(4) -> Err; dir is empty afterwards */ }
#[test] fn same_name_twice_in_batch() { /* two Incoming "x.zip" finished -> "x.zip","x (1).zip", contents distinct */ }
#[test] fn finish_never_replaces_existing() { /* pre-create "x.zip"="old"; finish new -> "x (1).zip"; "x.zip" still "old" */ }
#[test] fn drop_removes_temp() { /* begin, write, drop -> dir empty */ }
#[cfg(unix)] #[test] fn received_file_not_executable() { /* final mode & 0o111 == 0 */ }
```

- [ ] **Step 2:** `cargo test store` → FAIL.
- [ ] **Step 3:** Implement as specified above.
- [ ] **Step 4:** `cargo test store` → PASS; `cargo check --target x86_64-pc-windows-gnu` → OK (the MOTW `#[cfg(windows)]` branch compiles).
- [ ] **Step 5: Commit** `feat: safe receive-to-folder (temp name, no overwrite, MOTW)` (README: a "Where files go" section: Downloads, ` (1)` naming, MOTW/SmartScreen, no exec bit).

### Task 3: Framing and Noise session (`proto.rs`)

**Files:**
- Create: `src/proto.rs` (+ tests)

**Interfaces:**
- Produces:
  - `pub fn write_frame(w: &mut impl Write, b: &[u8]) -> io::Result<()>` / `pub fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>>` (u16 BE length)
  - `pub struct Session<S: Read + Write> { s: S, t: snow::TransportState }`
  - `impl<S: Read + Write> Session<S> { pub fn client(s: S, psk: &[u8; 32]) -> Result<Self, String>; pub fn server(s: S, psk: &[u8; 32]) -> Result<Self, String>; pub fn send(&mut self, msg: &Msg) -> Result<(), String>; pub fn recv(&mut self) -> Result<Msg, String> }` — `client` writes `b'S'` first; `server` expects the caller already consumed the mode byte.
  - `pub enum Msg { Batch { count: u16, from: String }, File { size: u64, name: String }, Data(Vec<u8>), End, Done { saved: u16 }, Fail(String) }` with `fn encode(&self) -> Vec<u8>` and `fn decode(b: &[u8]) -> Result<Msg, String>` (layout from "Wire protocol")
  - `pub const CHUNK: usize = 60_000;`

- [ ] **Step 1: Write failing tests** (use `std::os::unix::net::UnixStream::pair()` is unix-only, so use a loopback `TcpListener` on port 0 + a thread):

```rust
#[test] fn msg_round_trip() { for m in [Msg::Batch{count:3,from:"pc".into()}, Msg::File{size:5,name:"a.zip".into()},
    Msg::Data(vec![1;CHUNK]), Msg::End, Msg::Done{saved:2}, Msg::Fail("disk full".into())] { assert_eq!(Msg::decode(&m.encode()).unwrap(), m) } }
#[test] fn decode_rejects_garbage() { assert!(Msg::decode(&[]).is_err()); assert!(Msg::decode(b"F\x00").is_err()); assert!(Msg::decode(b"Z").is_err()); }
#[test] fn session_same_psk_talks() { /* client sends File, server recv == it; server sends Done, client recv == it */ }
#[test] fn session_wrong_psk_fails() { /* server(psk [1;32]) vs client([2;32]): server() returns Err */ }
#[test] fn bytes_on_wire_are_encrypted() { /* record the client's stream bytes; the plaintext name "secret-name.zip" does not appear */ }
```

- [ ] **Step 2:** `cargo test proto` → FAIL.
- [ ] **Step 3:** Implement; derive `Debug, PartialEq` on `Msg`. Noise builder as in synergy-audio's `noise()` with `.psk(0, psk).prologue(b"synergy-dragndrop v1")`.
- [ ] **Step 4:** `cargo test proto` → PASS.
- [ ] **Step 5: Commit** `feat: framed Noise session and messages` (README unchanged; the module doc comment at the top of `proto.rs` documents the wire protocol as in synergy-audio's header comment).

### Task 4: Pairing (`pair.rs`)

**Files:**
- Create: `src/pair.rs` (+ tests)

**Interfaces:**
- Consumes: `read_frame`/`write_frame` (Task 3).
- Produces:
  - `pub fn new_code() -> String` — 8 digits; `pub fn show_code(c: &str) -> String` → `"1234-5678"`; `pub fn parse_code(s: &str) -> Result<String, String>` (drops `-` and spaces, needs exactly 8 digits; error text like synergy-audio's)
  - `pub fn host(s: &mut (impl Read + Write), code: &str) -> Result<[u8; 32], String>` — caller already read `b'P'`
  - `pub fn join(s: &mut (impl Read + Write), code: &str) -> Result<[u8; 32], String>` — writes `b'P'` first
  - Wrong code: both sides return `Err` containing `"wrong code"`.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn same_code_same_secret() { /* loopback; host(code) and join(code) both Ok and equal */ }
#[test] fn wrong_code_fails_both_sides() { /* host("12345678"), join("87654321"): both Err contain "wrong code" */ }
#[test] fn code_format() { assert_eq!(show_code("12345678"), "1234-5678"); assert_eq!(parse_code("1234-5678").unwrap(), "12345678");
    assert!(parse_code("1234").is_err()); assert!(parse_code("abcd-efgh").is_err()); }
```

- [ ] **Step 2:** `cargo test pair` → FAIL.
- [ ] **Step 3:** Implement per "Wire protocol" (adapt synergy-audio's `spake`/`digest`; TCP needs no resend logic).
- [ ] **Step 4:** `cargo test pair` → PASS.
- [ ] **Step 5: Commit** `feat: SPAKE2 pairing over TCP`.

### Task 5: Transfer and headless CLI (`transfer.rs`, `main.rs`)

Milestone: a working `pair` / `send` / `listen` CLI on both machines before any GUI.

**Files:**
- Create: `src/transfer.rs` (+ tests)
- Modify: `src/main.rs` (wire subcommands), `README.md`

**Interfaces:**
- Consumes: `Peer`, `load_peer`, `save_peer`, `resolve`, `this_pc_name` (Task 1); `store::{sanitize_name, begin, Incoming}` (Task 2); `Session`, `Msg`, `CHUNK` (Task 3); `pair::{host, join}` (Task 4).
- Produces:
  - `pub fn check_files(paths: &[PathBuf]) -> Result<(), String>` — every path must be an existing regular file; folder → `not sent: <name> is a folder, zip it first`; missing → `not sent: <path> not found`. Runs **before** connecting.
  - `pub fn send_files(peer: &Peer, paths: &[PathBuf]) -> Result<String, String>` — `Ok("N files sent to <host>")`; connect with 3 s timeout, failure → exactly `not sent: other PC unreachable`; handshake EOF → `not sent: the other PC refused (pairing doesn't match: pair again)`; `Fail(r)` → `not sent: <r>`. On a write error, try to read a pending `Fail` before reporting.
  - `pub struct Received { pub from: String, pub files: Vec<PathBuf> }`
  - `pub struct Listener { sock: TcpListener, pub pairing: Arc<Mutex<Option<(String, Instant)>>>, downloads: PathBuf }`
  - `impl Listener { pub fn bind(port: u16, downloads: PathBuf) -> Result<Self, String>; pub fn bind_with(port: u16, downloads: PathBuf, config_dir: PathBuf) -> Result<Self, String>; pub fn run(&self, on_event: impl Fn(Event) + Send + Sync) -> ! }` (tests point `config_dir` at a temp dir; `bind` passes `config_dir()`)
  - `run`: one thread per connection; reads mode byte; `b'P'` only while `pairing` is `Some` and not expired (then clears it: one attempt), saves `Peer { host: <remote ip>, secret }`; `b'S'` → reloads the peer file each connection (so a new pairing applies without restart), `Session::server`, receives one batch into `downloads` via `store`, replies `Done`/`Fail`.
  - `pub enum Event { Received(Received), Paired(String), Refused(IpAddr), Error(String) }`
  - Bind error `AddrInUse` → `port 24851 is busy: synergy-dragndrop is probably already running (use its tray menu "Pair…", or quit it first)`.
  - `pub fn serve_one(stream: TcpStream, secret: &[u8;32], dir: &Path) -> Result<Received, String>` (the per-connection session part, testable without a peer file).
- CLI (`main.rs`):
  - `pair` — bind, set pairing mode for 120 s, print `Pairing code: 1234-5678` and `On the other PC run:  synergy-dragndrop pair <this PC's address> 1234-5678`, exit after one attempt.
  - `pair <host> <code>` — `join`, save `Peer { host, secret }`, print `Paired with <host>.`
  - `peer <host>` — change the saved address, keep the secret (for when the other PC's IP changes).
  - `unpair` — remove the peer file.
  - `send <file>…` — `check_files`, `send_files`, print the result (exit 1 on error).
  - `listen` — bind, run, print each `Event` (`2 files received from <name>: …`).

- [ ] **Step 1: Write failing tests** (loopback listener on port 0, temp Downloads dir):

```rust
#[test] fn sends_two_files() { /* send a.txt + b.bin (200 KB, > CHUNK) -> dir has both, bytes equal; Ok contains "2 files sent" */ }
#[test] fn folder_refused_before_connecting() { assert_eq!(check_files(&[dir]).unwrap_err(), format!("not sent: {} is a folder, zip it first", name)); }
#[test] fn unreachable_fails_fast() { /* Peer{host:"127.0.0.1:1"} -> Err == "not sent: other PC unreachable", elapsed < 4 s */ }
#[test] fn wrong_secret_refused() { /* server secret [1;32], client [2;32] -> client Err contains "refused"; dir empty */ }
#[test] fn truncated_stream_leaves_nothing() { /* hand-driven client: Batch, File{size:1000}, Data(10 bytes), then drop -> serve_one Err; dir empty (no temp, no file) */ }
#[test] fn receiver_error_reaches_sender() { /* dir that doesn't exist -> client Err starts with "not sent: " */ }
#[test] fn hostile_name_stays_in_dir() { /* File{name:"../evil"} -> saved as dir/"evil" */ }
#[test] fn pairing_only_when_enabled() { /* Listener with pairing None: join() Err; set Some(code): join Ok, then pairing is None again */ }
#[test] fn busy_port_message() { /* bind twice same port -> Err contains "already running" */ }
```

- [ ] **Step 2:** `cargo test transfer` → FAIL.
- [ ] **Step 3:** Implement. Stream timeouts: `set_read_timeout(30 s)`, `set_write_timeout(30 s)`. `downloads` = `dirs::download_dir()`, error `can't find your Downloads folder` if `None`.
- [ ] **Step 4:** `cargo test` (full) → PASS; `cargo check --target x86_64-pc-windows-gnu` → OK.
- [ ] **Step 5:** README: "Pairing" and "Command line" sections (all subcommands, port 24851, firewall note).
- [ ] **Step 6: Commit** `feat: send/listen/pair CLI`.
- [ ] **Step 7: Hands-on check A** (user, see "Hands-on checks" below) before Task 6.

### Task 6: Notifications (`notify.rs`)

**Files:**
- Create: `src/notify.rs`
- Modify: `Cargo.toml` (`[target.'cfg(target_os = "linux")'.dependencies] notify-rust = "4"`, `[target.'cfg(windows)'.dependencies] tauri-winrt-notification = "0.8"`), `src/main.rs`, `README.md`

**Interfaces:**
- Produces:
  - `pub fn received_text(n: usize, from: &str) -> String` → `"1 file received from X"` / `"3 files received from X"` (pure, tested)
  - `pub fn info(text: &str)` and `pub fn error(text: &str)` — fire-and-forget toast, title `synergy-dragndrop`.
  - `pub fn received(r: &transfer::Received)` — toast with `received_text`; clicking opens the Downloads folder. Linux: notify-rust `.action("default", "Open Downloads")` + `wait_for_action` in a spawned thread, open with `xdg-open <dir>`. Windows: `Toast::new(Toast::POWERSHELL_APP_ID)…on_activated(…)` opening `explorer.exe <dir>`. Failure to show a notification is logged, never fatal.
  - `pub fn open_folder(dir: &Path)`.
- CLI: `send` shows `info(ok)`/`error(err)` as well as printing (Send To has no terminal); `listen` calls `notify::received`.

- [ ] **Step 1: Failing test** `notify::tests::received_text_plural`: `assert_eq!(received_text(1,"pc"),"1 file received from pc"); assert_eq!(received_text(3,"pc"),"3 files received from pc");`
- [ ] **Step 2:** `cargo test notify` → FAIL. **Step 3:** implement. **Step 4:** `cargo test notify` PASS; `cargo check --target x86_64-pc-windows-gnu` OK.
- [ ] **Step 5:** Local manual check on Linux: `cargo run -- send /nonexistent` shows an error notification.
- [ ] **Step 6: Commit** `feat: desktop notifications` (README: "Notifications").

### Task 7: Drop window (`gui.rs`, part 1) — Wayland spike first

**Files:**
- Create: `src/gui.rs`
- Modify: `Cargo.toml` (`tao = "0.37"`), `src/main.rs` (no args → `gui::run()`), `.github/workflows/ci.yml` (Linux: `sudo apt-get install -y libgtk-3-dev`), `README.md`

**Interfaces:**
- Consumes: `check_files`, `send_files`, `load_peer` (Tasks 1, 5); `notify::{info, error}` (Task 6).
- Produces: `pub fn run() -> Result<(), String>`; `enum UserEvent { SendDone(Result<String, String>), Received(Received), Paired(String), Tray(…) }` sent through `EventLoopProxy` (later tasks add variants).

- [ ] **Step 1: Spike.** A 160×160 decorated, resizable-off window titled `Drop files to send`, `with_always_on_top(true)` (honoured on Windows; ignored on Wayland), positioned bottom-right on Windows via the monitor size. Print every `WindowEvent::DroppedFile` path. Run `cargo run` on KDE Plasma Wayland and drop two files from Dolphin: both paths printed. **If tao does not deliver drops on Wayland, stop and report** (fallback to decide with the user: `gtk4` for the Linux window).
- [ ] **Step 2:** Batch drops: push each `DroppedFile` into `pending: Vec<PathBuf>`; on `Event::MainEventsCleared` (tao's end-of-batch event), if non-empty, take it and spawn a thread running `check_files` + `send_files`, result back as `UserEvent::SendDone`.
- [ ] **Step 3:** Show state in the window title (no drawing crate): `Drop files to send` → `Sending 2 files…` → `Sent ✓` / `Not sent` (3 s, then back); `HoveredFile` → `Release to send`. `info`/`error` notification on `SendDone`. Close button hides the window (doesn't quit).
- [ ] **Step 4:** Pure helper + test: `fn title_for(state: &DropState) -> String`, test `gui::tests::titles` asserting the four strings above.
- [ ] **Step 5:** `cargo test gui` PASS; `cargo check --target x86_64-pc-windows-gnu` OK; CI green on both.
- [ ] **Step 6: Commit** `feat: drop window` (README: "Drop window", including the KDE tip: right-click title bar → More Actions → Keep Above Others, or a Window Rule matching title `Drop files to send` with position + Keep above).

### Task 8: Tray and in-process listener (`gui.rs`, part 2)

**Files:**
- Modify: `Cargo.toml` (`tray-icon`: `[target.'cfg(target_os = "linux")'.dependencies] tray-icon = { version = "0.26", default-features = false, features = ["ksni"] }`, Windows: `tray-icon = "0.26"`), `src/gui.rs`, `README.md`

**Interfaces:**
- Consumes: `Listener::{bind, run}`, `Event` (Task 5); `pair::{new_code, show_code}` (Task 4); `notify::received` (Task 6).
- Produces: none for later tasks.

- [ ] **Step 1:** `run()` binds the `Listener` first (busy port → `error` notification with the Task 5 message, exit 1: single instance), runs it on a thread, forwards `Event`s to the event loop via the proxy.
- [ ] **Step 2:** Tray icon: 32×32 RGBA generated in code (filled rounded square; grey = not paired, green = paired/idle, blue = sending/receiving, red for 5 s after an error). Tooltip `synergy-dragndrop: paired with <host>` / `not paired`. Pure helper `fn icon_rgba(state: TrayState) -> Vec<u8>`; test `gui::tests::icon_size` (len == 32*32*4).
- [ ] **Step 3:** Menu: `Show drop window` / `Hide drop window` (toggles), `Pair…` (sets `listener.pairing = Some((new_code(), now+120s))`, shows `Pairing code 1234-5678` in the window title and as a notification with `On the other PC run: synergy-dragndrop pair <this PC> 1234-5678`), `Quit`. `Event::Paired` → notification `Paired with <ip>`; `Event::Received` → `notify::received`.
- [ ] **Step 4:** `cargo test` PASS; `cargo check --target x86_64-pc-windows-gnu` OK; `cargo run` on KDE: tray icon visible, menu works, a `send` from a second terminal (`cargo run -- send x`, after pairing loopback via `peer 127.0.0.1`) produces the received notification.
- [ ] **Step 5: Commit** `feat: tray icon and listener in the tray app` (README: "Tray icon").

### Task 9: Windows integration

**Files:**
- Modify: `src/main.rs`
- Create: `install-autostart.ps1`, `uninstall-autostart.ps1`
- Modify: `README.md`

- [ ] **Step 1:** `#![cfg_attr(windows, windows_subsystem = "windows")]` so the tray app and Send To open no console; in `main`, on Windows when args are given, call `AttachConsole(ATTACH_PARENT_PROCESS = u32::MAX)` (raw `extern "system"` like synergy-audio's `FreeConsole`) so terminal runs still print.
- [ ] **Step 2:** `install-autostart.ps1` (ASCII only, header comment in sibling style), param `-Peer <host>` optional (calls `peer` if given):
  - Stop running instances; copy exe to `%LOCALAPPDATA%\synergy-dragndrop\`; `Unblock-File`.
  - Scheduled task `synergy-dragndrop` at logon, Interactive, Limited, 10 s delay, restart on failure, running the exe with no args (tray app) — sibling's settings block.
  - Send To shortcut `%APPDATA%\Microsoft\Windows\SendTo\Send to other PC.lnk` → exe with argument `send` (Explorer appends the selected files) via `WScript.Shell` `CreateShortcut`.
  - Firewall: if `Get-NetFirewallRule -DisplayName "synergy-dragndrop (TCP 24851)"` is missing, one `Start-Process powershell -Verb RunAs -Wait` running `New-NetFirewallRule -DisplayName "synergy-dragndrop (TCP 24851)" -Direction Inbound -Protocol TCP -LocalPort 24851 -Program <exe> -Profile Private,Public -Action Allow`. Verify afterwards; print a hint if the user declined UAC.
  - Final status lines like the sibling's.
- [ ] **Step 3:** `uninstall-autostart.ps1`: remove task, shortcut, firewall rule (one UAC prompt, only if the rule exists), stop process; keep config and Downloads.
- [ ] **Step 4:** `cargo check --target x86_64-pc-windows-gnu` OK; CI windows job green.
- [ ] **Step 5: Commit** `feat: Windows install (autostart, Send To, firewall)` (README: Windows install steps).

### Task 10: Linux integration

**Files:**
- Create: `install-autostart.sh`, `uninstall-autostart.sh`
- Modify: `README.md`

- [ ] **Step 1:** `install-autostart.sh` (sibling's structure: build from source if `Cargo.toml`, else use the bundled binary; temp-name install to `~/.local/bin/synergy-dragndrop`):
  - XDG autostart `~/.config/autostart/synergy-dragndrop.desktop` (`Exec=%h…` is not valid there: write the absolute path; `X-GNOME-Autostart-enabled=true`). Start it now with `setsid -f` if not running. (XDG autostart, not a systemd service: the tray app needs the graphical session; Plasma runs autostart entries as systemd units itself.)
  - Dolphin: `~/.local/share/kio/servicemenus/synergy-dragndrop.desktop` (`Type=Service`, `MimeType=application/octet-stream;`, `X-KDE-ServiceTypes=KonqPopupMenu/Plugin`, action `Name=Send to other PC`, `Exec=<bin> send %F`), `chmod +x` (Plasma 6 requires it).
  - Nautilus: `~/.local/share/nautilus/scripts/Send to other PC` (`#!/bin/sh` + `exec <bin> send "$@"`), `chmod +x`.
  - Firewall: sibling's ufw/firewalld block with `tcp` instead of `udp`, port 24851.
- [ ] **Step 2:** `uninstall-autostart.sh`: stop the process, remove the three entries and the binary; leave config and firewall rule (print the command to close it).
- [ ] **Step 3:** `bash -n` both scripts; `shellcheck` if installed; run `./install-autostart.sh` locally → Dolphin shows "Send to other PC" on a file, the tray icon appears.
- [ ] **Step 4: Commit** `feat: Linux install (autostart, Dolphin/Nautilus menus, firewall)` (README: Linux install steps).

### Task 11: Release workflow and final docs

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `README.md`, `DESIGN.md`

- [ ] **Step 1:** `release.yml` from the sibling: version guard; `linux` (ubuntu-22.04, `libgtk-3-dev`, tarball `synergy-dragndrop-vX.Y.Z-linux-x86_64.tar.gz` with binary, both `.sh` scripts, README, LICENSE); `windows` (`+crt-static`, zip `synergy-dragndrop-vX.Y.Z-windows-x86_64.zip` with exe, both `.ps1`, README, LICENSE at the zip root); `release` job with SHA256SUMS. No macOS.
- [ ] **Step 2:** README final pass in synergy-audio's style: CI badge, idea paragraph, **Status** line, "Download and install" (Linux first, then Windows, then pairing: tray "Pair…" on one, `pair <host> <code>` on the other), daily use (drop window, right-click), where files go, notifications, troubleshooting (firewall, "unreachable", port busy, Wayland Keep Above, SmartScreen "More info → Run anyway"), uninstall, build from source, how it works (protocol summary), security notes. Every path, name, port and command checked against the code.
- [ ] **Step 3:** DESIGN.md "Decided": add the builder's choices with date — direct-connect `send` for Send To; crates (tao, tray-icon/ksni, notify-rust, tauri-winrt-notification); pairing (one side shows the code via tray or `pair`, other runs `pair <host> <code>`; peer file in the config dir); XDG autostart on Linux.
- [ ] **Step 4:** Full `cargo test`; `cargo clippy` clean; `cargo check --target x86_64-pc-windows-gnu`; CI green.
- [ ] **Step 5: Commit** `docs: README, release workflow, design decisions`; push once a remote exists; tag `v0.1.0` only after Hands-on check B passes.

---

## Hands-on checks (for the user)

You do these; no code reading. Report what you saw at each "Look for".

**Check A — after Task 5 (terminal only, both PCs).** Build: Linux `cargo build --release`; Windows: the agent gives you the CI artifact or `synergy-dragndrop.exe`.
1. Linux: open TCP 24851 (`sudo ufw allow 24851/tcp` if ufw is on). Run `./synergy-dragndrop pair`. Look for: `Pairing code: NNNN-NNNN`.
2. Windows, PowerShell in the exe's folder: `.\synergy-dragndrop.exe pair <linux-ip> NNNN-NNNN`. Look for: `Paired with …` on both. (Windows Firewall may pop up; allow on Private and Public.)
3. Linux: `./synergy-dragndrop listen`. Windows: `.\synergy-dragndrop.exe send C:\some\file.zip C:\some\shot.png`. Look for: `2 files sent` on Windows, both files in Linux `~/Downloads`, a notification on Linux.
4. Send the same file again. Look for: `file (1).zip` next to the first; the first unchanged.
5. Windows: `send C:\Windows` (a folder). Look for: `not sent: Windows is a folder, zip it first`, nothing arrives.
6. Stop `listen` (Ctrl+C), send again. Look for: `not sent: other PC unreachable` within about 3 seconds.
7. Reverse: Windows `.\synergy-dragndrop.exe listen`, Linux `./synergy-dragndrop send <file.exe>`. Look for: the file in Windows Downloads; double-clicking it shows SmartScreen / "Windows protected your PC" (More info → Run anyway works). Right-click → Properties shows "This file came from another computer".
8. A third device (or `unpair` then `pair` with a wrong code on one side): Look for: `wrong code` / `refused`, nothing saved.

**Check B — after Task 10 (installed, both PCs).**
1. Linux: `./install-autostart.sh` (sudo prompt only for the firewall). Windows: `powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1`. Look for: one UAC prompt on Windows, tray icons on both.
2. Pair from the tray: Linux tray → Pair…, then on Windows the shown command. Look for: tray icons turn green on both.
3. Drag two screenshots onto the Linux drop window. Look for: title `Sending 2 files…` then `Sent ✓`; on Windows a toast "2 files received from <linux name>"; clicking it opens Downloads.
4. Windows Explorer: right-click a `.zip` → Send to → Send to other PC. Look for: no console window flashes; Linux notification; file in Downloads.
5. Dolphin: right-click a file → Send to other PC. Look for: Windows toast.
6. Drop a folder on the drop window. Look for: "zip it first" notification.
7. Reboot both (or log out/in). Look for: tray icons back without doing anything; step 3 still works.
8. KDE: try to keep the drop window above others (title bar right-click → More Actions → Keep Above Others). Look for: it stays on top; tell us if the default size/position is annoying.
