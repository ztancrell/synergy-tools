//! Embeds the git short hash as GIT_HASH (CI: GITHUB_SHA; else `git rev-parse`; else "unknown").
fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    let hash = std::env::var("GITHUB_SHA").ok().map(|s| s.chars().take(7).collect()).or_else(|| {
        let out = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    });
    println!("cargo:rustc-env=GIT_HASH={}", hash.filter(|h: &String| !h.is_empty()).unwrap_or_else(|| "unknown".into()));
}
