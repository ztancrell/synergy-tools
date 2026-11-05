mod config;
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

/// This PC's LAN address as the other PC would reach it, else the PC name.
fn local_address() -> String {
    // ponytail: picks the interface of the default route; a multi-homed PC may need the user to substitute.
    let probe = || {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("192.0.2.1:9").ok()?; // no packet is sent
        Some(s.local_addr().ok()?.ip().to_string())
    };
    probe().unwrap_or_else(config::this_pc_name)
}

fn show(e: &Event) -> String {
    match e {
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
    println!("On the other PC run:  synergy-dragndrop pair {} {}", local_address(), pair::show_code(&code));
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
        [] => return Err("the tray app is not built yet; use the subcommands below\n".to_string() + USAGE),
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
