//! `cargo xtask <op>` — one entrypoint for the recurring operations of this
//! project, standardized under cargo. It is a thin task runner: ceremony ops
//! forward to the Python continuity ceremony (with the repo venv + PYTHONPATH
//! wired in, so the import path never bites), bundle ops forward to the
//! `comms-verify` binary, and `anchor`/`test` drive the existing scripts.
//!
//! Run from the `rust/` directory: `cargo xtask status`, `cargo xtask open
//! --auto-derive ...`, `cargo xtask anchor`, `cargo xtask test`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // CARGO_MANIFEST_DIR = <repo>/rust/xtask; the repo root is two levels up.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((op, rest)) = args.split_first() else {
        usage();
        std::process::exit(1);
    };

    let code = match op.as_str() {
        // Session ceremony (Python), forwarded verbatim.
        "status" | "open" | "close" | "sign" | "finalize" | "destroy-key" | "verify"
        | "mint" | "new-session" | "sign-transcript" => ceremony(&repo, op, rest),
        // Friendlier name for the prose renderer.
        "log" => ceremony(&repo, "log-render", rest),
        // Pack + seal + verify + inspect the continuity store (the anchor artifact).
        "anchor" => anchor(&repo, rest),
        // Passthrough to the sneakernet binary: `cargo xtask bundle inspect b.cbor`.
        "bundle" => bundle(&repo, rest),
        // Both test suites.
        "test" => test(&repo),
        "help" | "-h" | "--help" => {
            usage();
            0
        }
        other => {
            eprintln!("xtask: unknown op '{other}'\n");
            usage();
            1
        }
    };
    std::process::exit(code);
}

fn usage() {
    eprintln!("cargo xtask <op> [args]  — recurring operations for Comms");
    eprintln!();
    eprintln!("session ceremony (Python, args forwarded):");
    eprintln!("  status                    where am I in the ceremony?");
    eprintln!("  open --auto-derive ...    mint + stage a new session log entry");
    eprintln!("  close --transcript F      the instance's closing rite");
    eprintln!("  sign --key <ssh-key>      historian countersigns pending items");
    eprintln!("  finalize                  seal signed pending items into the store");
    eprintln!("  destroy-key               release the session seed (Article 1)");
    eprintln!("  verify                    walk + verify the continuity store (the door)");
    eprintln!("  log [--session-num N]     render the trial-log.md entry for a session");
    eprintln!();
    eprintln!("toolkit:");
    eprintln!("  anchor [key] [out]        pack+seal+verify the store into one bundle");
    eprintln!("  bundle <args>             run comms-verify (verify/inspect/pack/...)");
    eprintln!("  test                      cargo test + pytest");
}

/// The repo venv's Python if present, else the system `python3`.
fn venv_python(repo: &Path) -> PathBuf {
    let v = repo.join(".venv/bin/python");
    if v.exists() {
        v
    } else {
        PathBuf::from("python3")
    }
}

fn run(cmd: &mut Command) -> i32 {
    match cmd.status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("xtask: failed to launch {:?}: {e}", cmd.get_program());
            1
        }
    }
}

fn ceremony(repo: &Path, sub: &str, rest: &[String]) -> i32 {
    // PYTHONPATH=<repo> so `from comms import ...` resolves the inner package.
    run(Command::new(venv_python(repo))
        .current_dir(repo)
        .env("PYTHONPATH", repo)
        .arg("scripts/continuity_ceremony.py")
        .arg(sub)
        .args(rest))
}

fn anchor(repo: &Path, rest: &[String]) -> i32 {
    run(Command::new("sh")
        .current_dir(repo)
        .arg("scripts/anchor_continuity_bundle.sh")
        .args(rest))
}

fn bundle(repo: &Path, rest: &[String]) -> i32 {
    let bin = repo.join("rust/target/release/comms-verify");
    if !bin.exists() {
        eprintln!("building comms-verify (release)...");
        let built = run(Command::new("cargo")
            .current_dir(repo.join("rust"))
            .args(["build", "--release"]));
        if built != 0 {
            return built;
        }
    }
    run(Command::new(bin).args(rest))
}

fn test(repo: &Path) -> i32 {
    let rust = run(Command::new("cargo").current_dir(repo.join("rust")).arg("test"));
    let py = run(Command::new(venv_python(repo))
        .current_dir(repo)
        .env("PYTHONPATH", repo)
        .args(["-m", "pytest", "-q"]));
    if rust == 0 && py == 0 {
        0
    } else {
        1
    }
}
