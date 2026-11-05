// No console window for the tray app and Send To; terminal runs re-attach to the parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod audio;
mod config;
mod tray;
mod notify;
mod pair;
mod proto;
mod store;
mod transfer;

use std::path::PathBuf;
use std::time::{Duration, Instant};
use transfer::{Event, Listener};

pub(crate) const PORT: u16 = 24851;

const USAGE: &str = "usage:
  synergy-tools                         tray app (drop window, receives files)
  synergy-tools pair                    show a code and wait for the other PC to pair
  synergy-tools pair <host> <code>      pair with the PC showing <code> (adds a partner)
  synergy-tools partners                list paired PCs
  synergy-tools partner <name> <host>   change a partner's address
  synergy-tools unpair <name>           forget a partner
  synergy-tools files send [--to <name>] <file>...
                                        send files to the online partner (--to if several are online)
  synergy-tools files listen            receive files without the tray app
  synergy-tools audio send <host[:port]> [--log <file>]
                                        stream this PC's sound to a partner (default port 24850);
                                        --log: status to <file> (emptied past 1 MiB), retry on errors
  synergy-tools audio receive [--buffer-ms N]
                                        play a partner's sound (default buffer 20 ms)
  synergy-tools audio unmute            undo the sender's auto-mute after it was killed
  synergy-tools --version";

fn downloads() -> Result<PathBuf, String> {
    dirs::download_dir().ok_or_else(|| "can't find your Downloads folder".to_string())
}

fn show(e: &Event) -> String {
    match e {
        Event::Receiving(from) => format!("receiving from {from}"),
        Event::Received(r) => {
            let names: Vec<_> = r.files.iter().map(|f| f.display().to_string()).collect();
            let n = names.len();
            format!("{n} file{} received from {}: {}", if n == 1 { "" } else { "s" }, r.from, names.join(", "))
        }
        Event::Paired(name) => format!("Paired with {name}."),
        Event::Refused(ip) => format!("refused connection from {ip}: not a paired PC"),
        Event::Error(e) | Event::PairFailed(e) => e.clone(),
    }
}

fn pair_host() -> Result<(), String> {
    let l = Listener::bind(PORT, downloads()?)?;
    let code = pair::new_code();
    println!("Pairing code: {}", pair::show_code(&code));
    println!("On the other PC run:  synergy-tools pair {} {}", config::local_address(), pair::show_code(&code));
    let pairing = l.pairing.clone();
    *pairing.lock().unwrap() = Some((code, Instant::now()));
    std::thread::spawn(|| {
        std::thread::sleep(transfer::PAIR_WINDOW + Duration::from_secs(1));
        eprintln!("pairing code expired; run `synergy-tools pair` again");
        std::process::exit(1);
    });
    l.run(move |e| {
        println!("{}", show(&e));
        match e {
            Event::Paired(_) => std::process::exit(0),
            // Only a failed pairing attempt ends the wait; stray connections don't.
            Event::PairFailed(_) => std::process::exit(1),
            _ => {}
        }
    })
}

fn send(to: Option<&str>, args: &[std::ffi::OsString]) -> Result<(), String> {
    let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    // Send To has no terminal, so the result also goes to a notification.
    let res = transfer::check_files(&paths).and_then(|_| {
        match transfer::target(transfer::online(&config::load_partners()?), to)? {
            transfer::Target::Nobody => Err("not sent: no partner online".into()),
            transfer::Target::One(p) => transfer::send_files(&p, &paths),
            transfer::Target::Several(ps) => Err(format!(
                "several partners online ({}): use --to <name>",
                ps.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
            )),
        }
    });
    match &res {
        Ok(m) => notify::info(m),
        Err(e) => notify::error(e),
    }
    println!("{}", res?);
    Ok(())
}

fn run(args: &[std::ffi::OsString]) -> Result<(), String> {
    // Lossy only for subcommand/host/code matching; `files send` paths keep their raw bytes.
    let lossy: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    match lossy.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--version"] => println!("synergy-tools {}", env!("CARGO_PKG_VERSION")),
        [] => tray::run()?,
        ["pair"] => pair_host()?,
        ["pair", host, code] => {
            let p = transfer::pair_with(host, code)?;
            println!("Paired with {} ({}).", p.name, p.os);
        }
        ["partners"] => {
            let ps = config::load_partners()?;
            if ps.is_empty() {
                println!("Not paired.");
            }
            for p in ps {
                println!("{}  {}  {}", p.name, p.os, p.host);
            }
        }
        ["partner", name, host] => {
            config::resolve(host, PORT)?;
            let mut ps = config::load_partners()?;
            let id = config::find(&ps, name)?.id;
            let p = ps.iter_mut().find(|p| p.id == id).expect("found above");
            p.host = host.to_string();
            let name = p.name.clone();
            config::save_partners(&ps)?;
            println!("{name} is now at {host}.");
        }
        ["unpair", name] => {
            let mut ps = config::load_partners()?;
            let p = config::find(&ps, name)?.clone();
            ps.retain(|q| q.id != p.id);
            config::save_partners(&ps)?;
            println!("Unpaired {}.", p.name);
        }
        ["files", "send", "--to", to, _, ..] => send(Some(to), &args[4..])?,
        ["files", "send", "--to", ..] => return Err(USAGE.into()),
        ["files", "send", _, ..] => send(None, &args[2..])?,
        ["files", "listen"] => {
            let dir = downloads()?;
            let l = Listener::bind(PORT, dir.clone())?;
            println!("Listening on port {PORT}, saving to {}", dir.display());
            l.run(|e| {
                println!("{}", show(&e));
                if let Event::Received(r) = &e {
                    notify::received(r);
                }
            })
        }
        ["audio", "send", target] => audio::run_send(target)?,
        ["audio", "send", target, "--log", path] => audio::run_send_logged(target, path)?,
        ["audio", "receive"] => audio::run_receive(audio::DEFAULT_BUFFER_MS)?,
        ["audio", "receive", "--buffer-ms", n] => match n.parse() {
            Ok(n) if n > 0 => audio::run_receive(n)?,
            _ => return Err(format!("--buffer-ms needs a positive integer, got {n:?}")),
        },
        ["audio", "unmute"] => audio::mute::restore(),
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

/// A GUI-subsystem exe has no console. With arguments (terminal run) attach to the parent's and,
/// unless stdout/stderr were redirected, point them at it so println!/eprintln! show up.
#[cfg(windows)]
fn attach_console() {
    use std::os::windows::io::IntoRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn GetStdHandle(which: u32) -> isize;
        fn SetStdHandle(which: u32, h: isize) -> i32;
    }
    unsafe {
        if AttachConsole(u32::MAX) == 0 {
            return; // no parent console (e.g. Send To): output is dropped
        }
        for which in [-11i32 as u32, -12i32 as u32] {
            let h = GetStdHandle(which);
            if h == 0 || h == -1 {
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                    SetStdHandle(which, f.into_raw_handle() as isize);
                }
            }
        }
    }
}

fn main() {
    #[cfg(windows)]
    if std::env::args_os().len() > 1 {
        attach_console();
    }
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
