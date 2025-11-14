use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use xshell::{cmd, Shell};

#[derive(Parser)]
#[command(name = "cargo-xtask", version, about = "Project automation tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Format Rust and frontend sources
    Fmt,
    /// Run lint and static analysis checks
    Check,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let project_root = project_root();
    let shell = Shell::new()?;
    let _ = shell.push_dir(project_root);

    match cli.command {
        Command::Fmt => run_fmt(&shell),
        Command::Check => run_check(&shell),
    }
}

fn run_fmt(shell: &Shell) -> Result<()> {
    let npm = npm_cmd();
    cmd!(shell, "cargo fmt --all")
        .run()
        .context("failed to run cargo fmt")?;
    cmd!(shell, "{npm} run format")
        .run()
        .context("failed to run npm format")?;
    Ok(())
}

fn run_check(shell: &Shell) -> Result<()> {
    let npm = npm_cmd();
    cmd!(shell, "cargo fmt --all -- --check")
        .run()
        .context("cargo fmt --check failed")?;
    cmd!(
        shell,
        "cargo clippy --workspace --all-targets --all-features -- -D warnings"
    )
    .run()
    .context("cargo clippy failed")?;
    cmd!(shell, "{npm} run lint")
        .run()
        .context("npm lint failed")?;
    cmd!(shell, "{npm} run format:check")
        .run()
        .context("npm format:check failed")?;
    Ok(())
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn npm_cmd() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}
