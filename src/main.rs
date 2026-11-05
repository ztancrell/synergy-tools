// ponytail: items are used by later tasks; drop this allow once the CLI is wired up.
#![allow(dead_code)]
mod config;

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

fn run(args: &[String]) -> Result<(), String> {
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--version"] => println!("synergy-dragndrop {}", env!("CARGO_PKG_VERSION")),
        [] | ["pair"] | ["pair", _, _] | ["peer", _] | ["unpair"] | ["send", _, ..] | ["listen"] => {
            return Err("not implemented yet".into())
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
