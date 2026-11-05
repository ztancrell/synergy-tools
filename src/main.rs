// No console window for the tray app and Send To; terminal runs re-attach to the parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod config;
mod gui;
mod notify;
mod pair;
mod proto;
mod store;
mod transfer;

use config::Peer;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use transfer::{Event, Listener};

pub(crate) const PORT: u16 = 24851;

const USAGE: &str = "usage:
  synergy-dragndrop                    tray app (drop window, receives files)
  synergy-dragndrop pair               show a code and wait for the other PC to pair
  synergy-dragndrop pair <host> <code> pair with the PC showing <code>
  synergy-dragndrop peer <host>        change the paired PC's address
  synergy-dragndrop unpair             forget the paired PC
  synergy-dragndrop send <file>...     send files to the paired PC
  synergy-dragndrop listen             receive files without the tray app
  synergy-dragndrop --version";

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
        Event::Paired(ip) => format!("Paired with {ip}."),
        Event::Refused(ip) => format!("refused connection from {ip}: not the paired PC"),
        Event::Error(e) | Event::PairFailed(e) => e.clone(),
    }
}

fn pair_host() -> Result<(), String> {
    let l = Listener::bind(PORT, downloads()?)?;
    let code = pair::new_code();
    println!("Pairing code: {}", pair::show_code(&code));
    println!("On the other PC run:  synergy-dragndrop pair {} {}", config::local_address(), pair::show_code(&code));
    let pairing = l.pairing.clone();
    *pairing.lock().unwrap() = Some((code, Instant::now()));
    std::thread::spawn(|| {
        std::thread::sleep(transfer::PAIR_WINDOW + Duration::from_secs(1));
        eprintln!("pairing code expired; run `synergy-dragndrop pair` again");
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

fn run(args: &[String]) -> Result<(), String> {
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--version"] => println!("synergy-dragndrop {}", env!("CARGO_PKG_VERSION")),
        [] => gui::run()?,
        ["pair"] => pair_host()?,
        ["pair", host, code] => {
            transfer::pair_with(host, code)?;
            println!("Paired with {host}.");
        }
        ["peer", host] => {
            config::resolve(host)?;
            let secret = config::load_peer()?.secret;
            config::save_peer(&Peer { host: host.to_string(), secret })?;
            println!("The paired PC is now at {host}.");
        }
        ["unpair"] => println!("{}", if config::remove_peer()? { "Unpaired." } else { "Not paired." }),
        ["send", _, ..] => {
            let paths: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
            // Send To has no terminal, so the result also goes to a notification.
            let res = transfer::check_files(&paths).and_then(|_| transfer::send_files(&config::load_peer()?, &paths));
            match &res {
                Ok(m) => notify::info(m),
                Err(e) => notify::error(e),
            }
            println!("{}", res?);
        }
        ["listen"] => {
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
