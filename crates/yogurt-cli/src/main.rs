mod commands;
mod data_dir;
mod logging;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// yogurt — local-first meeting copilot.
#[derive(Parser, Debug)]
#[command(name = "yogurt", version = env!("YOGURT_VERSION_LINE"), about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Launch the local server and open the browser.
    Start(StartArgs),
    /// Print diagnostic info (rust, macOS, perms, providers, models) + repair actions.
    Doctor(DoctorArgs),
    /// Control a running yogurt instance: status, meetings, detection, windows.
    Ctl(commands::ctl::CtlArgs),
}

#[derive(clap::Args, Debug)]
struct StartArgs {
    /// TCP port to bind.
    #[arg(long, default_value_t = 7878)]
    port: u16,
    /// Do not auto-open the browser on start.
    #[arg(long)]
    no_open: bool,
    /// Run in dev mode (proxies non-API routes to Vite on :5173).
    #[arg(long)]
    dev: bool,
    /// Directory for the SQLite database, keys.json, notes, and session token (default `~/.yogurt`).
    /// Models stay under `~/.yogurt` regardless. Also settable
    /// via `$YOGURT_DATA_DIR`; this flag wins if both are given.
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,
    /// Voice shortcut for this instance, replacing the one in Settings, such as `right_option+m`.
    /// A dev instance (`--dev`, or a data dir other than `~/.yogurt`) defaults to `right_option+m`,
    /// so it never answers the main yogurt's shortcut.
    #[arg(long, value_name = "KEY", value_parser = parse_shortcut)]
    voice_shortcut: Option<String>,
}

/// Replaces this process with the updated binary, same arguments, same PID. Returns only if exec fails.
/// Keeping the PID keeps the terminal macOS credits with the mic and Screen Recording grants.
fn restart_into(exe: &std::path::Path) {
    use std::os::unix::process::CommandExt;
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // The open tab reloads itself, so a second tab would be noise.
    if !args.iter().any(|a| a == "--no-open") {
        args.push("--no-open".into());
    }
    let err = std::process::Command::new(exe).args(args).exec();
    eprintln!(
        "Error: yogurt updated but could not restart into {}: {err}. Run `yogurt start` again.",
        exe.display()
    );
}

fn parse_shortcut(s: &str) -> Result<String, String> {
    s.parse::<yogurt_voice::Hotkey>()
        .map(|_| s.to_string())
        .map_err(|e| e.to_string())
}

#[derive(clap::Args, Debug)]
struct DoctorArgs {
    /// Emit diagnostics as JSON.
    #[arg(long)]
    json: bool,
    /// Reset Screen Recording TCC permission for ai.yogurt.app (forces re-prompt).
    #[arg(long)]
    reset_screen_recording: bool,
    /// Check whether port 7878 is in use.
    #[arg(long)]
    check_port: bool,
    /// Re-download a whisper.cpp model (e.g. small.en) by deleting the local copy.
    #[arg(long, value_name = "MODEL")]
    redownload_model: Option<String>,
    /// Report the database under this directory instead of `~/.yogurt`.
    /// Also settable via `$YOGURT_DATA_DIR`; this flag wins if both are given.
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    // SET-11 (Phase 5 / Plan 05-02): load `.env.local` ONLY in dev mode.
    //
    // The `--dev` check is done via raw `std::env::args` BEFORE `Cli::parse`
    // so the loaded env vars are visible to the parser (clap consults the
    // environment for any `#[arg(env = ...)]` defaults) and to
    // `yogurt-server`'s `bootstrap::seed_from_env` later in the boot chain.
    //
    // Release builds invoked WITHOUT `--dev` MUST NOT touch `.env.local` —
    // brew users never have one, and reading a sibling file would be a
    // surprise. The guard is the raw arg check, not the clap-parsed value,
    // so the file is read at most once regardless of clap configuration.
    //
    // Errors are silently ignored: a missing or unreadable `.env.local` is
    // a normal case (developer hasn't created one yet).
    if std::env::args().any(|a| a == "--dev") {
        let _ = dotenvy::from_filename(".env.local");
    }

    // LO-02: parse args BEFORE installing the tracing subscriber so
    // --help / --version paths never have a chance to emit a startup log
    // line. Any future `tracing::info!` added before `Cli::parse()` would
    // otherwise leak into help output.
    let cli = Cli::parse();

    let log_guard = logging::init(
        matches!(cli.command, Cmd::Start(_)),
        match &cli.command {
            Cmd::Start(args) => args.data_dir.clone(),
            _ => None,
        },
    );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    match cli.command {
        Cmd::Start(args) => {
            // AppKit must own the main thread for the Voice event tap and pill, so the server runs beside it.
            // Whichever way the server ends, the process ends with it.
            std::thread::Builder::new()
                .name("yogurt-server".into())
                .stack_size(8 << 20)
                .spawn(move || {
                    let result = rt.block_on(commands::start::run(commands::start::StartArgs {
                        port: args.port,
                        no_open: args.no_open,
                        dev: args.dev,
                        data_dir: args.data_dir,
                        voice_shortcut: args.voice_shortcut,
                    }));
                    if let Err(e) = &result {
                        eprintln!("Error: {e:?}");
                    }
                    // `exit` skips destructors, and the guard's drop flushes the log file.
                    drop(log_guard);
                    if result.is_ok() {
                        if let Some(exe) = yogurt_server::update::restart_target() {
                            restart_into(&exe);
                        }
                    }
                    std::process::exit(i32::from(result.is_err()));
                })?;
            #[cfg(target_os = "macos")]
            yogurt_voice::run_app();
            #[cfg(not(target_os = "macos"))]
            loop {
                std::thread::park();
            }
        }
        Cmd::Doctor(args) => rt.block_on(commands::doctor::run(commands::doctor::DoctorArgs {
            json: args.json,
            reset_screen_recording: args.reset_screen_recording,
            check_port: args.check_port,
            redownload_model: args.redownload_model,
            data_dir: args.data_dir,
        })),
        Cmd::Ctl(args) => {
            // CLI-4: ctl formats its own `error: ... / help: ...` pair and
            // picks its own exit code (0 success, 1 business-logic error;
            // clap already handles usage errors with exit 2 before this
            // ever runs) -- std::process::exit rather than bubbling an
            // anyhow::Result up through main's generic "Error: {e}" tail.
            let code = rt.block_on(commands::ctl::run(args));
            std::process::exit(code);
        }
    }
}
