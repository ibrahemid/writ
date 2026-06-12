use std::ffi::OsString;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process;

use clap::Parser;
use writ_cli::{resolve_targets, stdin_file_path, OpenTarget};

#[derive(Parser)]
#[command(
    name = "writ",
    about = "Open files, folders, or piped input in Writ",
    version
)]
struct Cli {
    /// Files or directories to open. Pass `-` or omit for piped stdin.
    #[arg(value_name = "PATH")]
    paths: Vec<OsString>,

    /// Title for the piped stdin buffer tab.
    #[arg(long, value_name = "TITLE")]
    title: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let is_pipe = !io::stdin().is_terminal();
    let effective_paths: Vec<OsString> = if is_pipe && cli.paths.is_empty() {
        vec![]
    } else {
        cli.paths
    };

    let target = match resolve_targets(&effective_paths, &cwd, cli.title.clone()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("writ: {e}");
            process::exit(1);
        }
    };

    match target {
        OpenTarget::Files(paths) => {
            for path in &paths {
                open_with_app(path);
            }
        }
        OpenTarget::Workspace(dir) => {
            open_with_app(&dir);
        }
        OpenTarget::Stdin { title } => {
            if !is_pipe {
                eprintln!("writ: no paths given and stdin is a terminal");
                eprintln!("Usage: writ [PATH...]  or pipe content via stdin");
                process::exit(1);
            }

            let mut content = String::new();
            if let Err(e) = io::stdin().read_to_string(&mut content) {
                eprintln!("writ: failed to read stdin: {e}");
                process::exit(1);
            }

            let piped_dir = resolve_piped_dir();
            if let Err(e) = std::fs::create_dir_all(&piped_dir) {
                eprintln!("writ: cannot create {}: {e}", piped_dir.display());
                process::exit(1);
            }

            let id = uuid::Uuid::new_v4().to_string();
            let dest = stdin_file_path(&piped_dir, &id, title.as_deref());

            if let Err(e) = std::fs::write(&dest, &content) {
                eprintln!("writ: cannot write to {}: {e}", dest.display());
                process::exit(1);
            }

            open_with_app(&dest);
        }
    }
}

fn resolve_piped_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".writ")
        .join("piped")
}

fn open_with_app(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let result = process::Command::new("open")
        .arg("-b")
        .arg("com.writ.editor")
        .arg(path)
        .status();

    #[cfg(target_os = "linux")]
    let result = process::Command::new("xdg-open").arg(path).status();

    #[cfg(target_os = "windows")]
    let result = process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .status();

    if let Err(e) = result {
        eprintln!("writ: failed to launch app for {}: {e}", path.display());
        process::exit(1);
    }
}
