//! cpp2rust-tools: the small programs of the C++ to Rust migration scenario, as one binary with
//! subcommands. Every subcommand reads and writes plain files (JSON, JSON Lines, YAML) so an
//! agent, a person and CI run the same code and read the same results.

pub mod catalog_lint;
pub mod depgraph;
pub mod fixture;
pub mod gate_check;
pub mod linked_check;
pub mod policy_scan;
pub mod run_cases;
#[doc(hidden)]
pub mod testutil;
pub mod trace_diff;
pub mod util;

use util::Fatal;

/// Subcommands in the order the scenario uses them: (name, one-line purpose, help text).
pub const COMMANDS: [(&str, &str, &str); 7] = [
    (
        "depgraph",
        "dependency graph and port order from compile_commands.json",
        depgraph::HELP,
    ),
    (
        "run-cases",
        "run surface cases against one binary; write observations as a trace",
        run_cases::HELP,
    ),
    (
        "trace-diff",
        "compare two traces (first divergence) or learn noise from two C++ runs",
        trace_diff::HELP,
    ),
    ("catalog-lint", "behavior catalog entries cite passing evidence", catalog_lint::HELP),
    (
        "gate-check",
        "run a gate's checks and compare with the plan's thresholds",
        gate_check::HELP,
    ),
    (
        "linked-check",
        "linker map shows Rust linked and the legacy C++ objects gone",
        linked_check::HELP,
    ),
    (
        "policy-scan",
        "no new allow, todo, static mut, disabled tests, edited recordings",
        policy_scan::HELP,
    ),
];

pub fn usage() -> String {
    let mut s = format!(
        "cpp2rust-tools {} - C++ to Rust migration tools\n\nUsage: cpp2rust-tools <command> [options]\n\n",
        env!("CARGO_PKG_VERSION")
    );
    for (name, what, _) in COMMANDS {
        s.push_str(&format!("  {name:<13} {what}\n"));
    }
    s.push_str("\n`cpp2rust-tools <command> --help` describes a command.\nExit codes: 0 pass, 1 check failed, 2 usage or input error, 3 (run-cases) not deterministic.");
    s
}

/// Run a subcommand; returns the process exit code.
pub fn run(argv: &[String]) -> i32 {
    let Some(cmd) = argv.first() else {
        eprintln!("{}", usage());
        return 2;
    };
    let rest = &argv[1..];
    let result = match cmd.as_str() {
        "depgraph" => depgraph::main(rest),
        "run-cases" => run_cases::main(rest),
        "trace-diff" => trace_diff::main(rest),
        "catalog-lint" => catalog_lint::main(rest),
        "gate-check" => gate_check::main(rest),
        "linked-check" => linked_check::main(rest),
        "policy-scan" => policy_scan::main(rest),
        "__fixture" => return fixture::main(rest),
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            return 0;
        }
        "--version" | "-V" => {
            println!("cpp2rust-tools {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        other => Err(Fatal(2, format!("unknown command {other:?}\n\n{}", usage()))),
    };
    match result {
        Ok(code) => code,
        Err(Fatal(0, _)) => {
            let help = COMMANDS.iter().find(|(n, _, _)| n == cmd).map_or("", |c| c.2);
            println!("{help}");
            0
        }
        Err(Fatal(code, msg)) => {
            eprintln!("cpp2rust-tools {cmd}: {msg}");
            code
        }
    }
}
