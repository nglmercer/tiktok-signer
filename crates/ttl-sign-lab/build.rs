use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let commit = std::env::var("GIT_COMMIT")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git(&workspace, &["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".into());
    let dirty =
        git(&workspace, &["status", "--porcelain"]).is_some_and(|output| !output.is_empty());
    println!("cargo:rustc-env=TTL_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=TTL_GIT_DIRTY={dirty}");
}

fn git(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
