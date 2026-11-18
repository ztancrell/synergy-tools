//! Embeds the git short hash as GIT_HASH (CI: GITHUB_SHA; else `git rev-parse`; else "unknown").
//! Windows target: also links the .exe icon (assets/synergy-tools.rc).
fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    let hash = std::env::var("GITHUB_SHA").ok().map(|s| s.chars().take(7).collect()).or_else(|| {
        let out = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    });
    println!("cargo:rustc-env=GIT_HASH={}", hash.filter(|h: &String| !h.is_empty()).unwrap_or_else(|| "unknown".into()));
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/synergy-tools.rc");
        println!("cargo:rerun-if-changed=assets/synergy-tools.ico");
        // The include dir lets rc.exe and windres both find the .ico named in the .rc.
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        match embed_resource::compile("assets/synergy-tools.rc", embed_resource::ParamsIncludeDirs([&assets])) {
            embed_resource::CompilationResult::NotAttempted(why) => println!("cargo:warning=no .exe icon: {why}"),
            r => r.manifest_optional().unwrap(),
        }
    }
}
