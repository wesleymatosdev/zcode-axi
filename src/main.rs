//! zcode-axi binary entry: machine-friendly control plane around the
//! OFFICIAL zcode runtime. See docs/protocol.md and docs/EXIT-CODES.md.

use clap::Parser;
use zcode_axi::cli::{Cli, Command, OutputOpts};
use zcode_axi::commands;
use zcode_axi::error::{exit, AxiError, AxiResult};
use zcode_axi::runtime::Runtime;

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let opts = OutputOpts::from_cli(cli.json, cli.pretty, cli.full);

    if matches!(cli.command, Command::FakeAppServer) {
        // In-process canned server used by unit tests over real stdio.
        return std::process::ExitCode::from(commands::fake_app_server_main() as u8);
    }

    let rt = match Runtime::discover(cli.zcode_bin.as_deref()) {
        Ok(rt) => rt,
        Err(e) => return fail(e),
    };

    let result: AxiResult<()> = match &cli.command {
        Command::Status => commands::cmd_status(&rt, opts),
        Command::Run {
            cwd,
            goal,
            max_turns,
        } => commands::cmd_run(&rt, opts, cwd, goal, *max_turns),
        Command::Sessions => commands::cmd_sessions(&rt, opts),
        Command::Inspect { id } => commands::cmd_inspect(&rt, opts, id),
        Command::Wait { id, timeout } => commands::cmd_wait(&rt, id, *timeout),
        Command::Resume {
            id,
            goal,
            max_turns,
        } => commands::cmd_resume(&rt, opts, id, goal, *max_turns),
        Command::Cancel { id } => commands::cmd_cancel(&rt, id),
        Command::FakeAppServer => unreachable!("handled above"),
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

/// Report an AxiError on stderr and map it to its documented exit code.
fn fail(e: AxiError) -> std::process::ExitCode {
    eprintln!("zcode-axi: {e} (exit {})", e.exit_code());
    if matches!(e, AxiError::UnsupportedByRuntime(_)) {
        eprintln!(
            "zcode-axi: this operation is unsupported by the runtime (exit {})",
            exit::UNSUPPORTED_BY_RUNTIME
        );
    }
    std::process::ExitCode::from(e.exit_code())
}
