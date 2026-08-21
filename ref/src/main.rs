//! als-ref — the ALS reference evaluator (ADR-0015).
//!
//! Protocol (the judge's seam; `scripts/conformance.py --legs ref` speaks it):
//!
//!   als-ref run <file.almd> --json
//!       {"exit": n, "stdout": "…", "stderr": "…"}          the program ran
//!       {"abstain": {"class": "…", "reason": "…"}}          not judged (ledger)
//!       {"error": "…"}                                      evaluator fault (red)
//!   als-ref run <file.almd>            plain: replays stdout/stderr, exits n;
//!                                      abstain → stderr line, exit 3; fault → exit 4
//!   als-ref parse <file.almd>          exit 0 iff the file parses (parser coverage)
//!   als-ref stdlib-index               implemented stdlib names, one per line
//!   als-ref --version
//!
//! Exit codes 3 and 4 are protocol codes in plain mode only; in `--json` mode
//! the verdict is in the document and the process exits 0 unless the
//! arguments themselves are wrong (exit 2).

// The crate is grown toward the whole ALS surface; syntax the parser keeps
// but the evaluator does not read yet (types, visibility, attributes) is
// intentionally retained — totality is measured by the abstain gate, not by
// dead-code lints.
#![allow(dead_code)]

mod ast;
mod eval;
mod lexer;
mod parser;
mod stdlib;
mod value;

use std::io::Write;

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn usage() -> ! {
    eprintln!("usage: als-ref run <file.almd> [--json] | als-ref parse <file.almd> | als-ref stdlib-index | als-ref --version");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    match args[0].as_str() {
        "--version" => {
            let pin = include_str!("../rust-toolchain.toml");
            let channel = pin
                .split_inclusive('\n')
                .find(|l| l.starts_with("channel"))
                .unwrap_or("?");
            println!(
                "als-ref {} (ADR-0015 reference evaluator; rustc pin {})",
                env!("CARGO_PKG_VERSION"),
                channel.trim_end_matches('\n')
            );
        }
        "stdlib-index" => {
            for n in stdlib::IMPLEMENTED {
                println!("{n}");
            }
        }
        "parse" => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("als-ref: cannot read {path}: {e}");
                std::process::exit(2)
            });
            match parser::parse_program(&src) {
                Ok(_) => println!("ok"),
                Err(e) => {
                    println!("parse error: line {}: {}", e.line, e.msg);
                    std::process::exit(1)
                }
            }
        }
        "run" => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let json = args.iter().skip(2).any(|a| a == "--json");
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("als-ref: cannot read {path}: {e}");
                std::process::exit(2)
            });
            let outcome = eval::run_source(&src);
            let so = std::io::stdout();
            let mut so = so.lock();
            match outcome {
                eval::Outcome::Ran {
                    exit,
                    stdout,
                    stderr,
                } => {
                    if json {
                        let _ = writeln!(
                            so,
                            "{{\"exit\": {exit}, \"stdout\": {}, \"stderr\": {}}}",
                            json_str(&stdout),
                            json_str(&stderr)
                        );
                    } else {
                        let _ = so.write_all(stdout.as_bytes());
                        let _ = so.flush();
                        let _ = std::io::stderr().write_all(stderr.as_bytes());
                        std::process::exit(exit);
                    }
                }
                eval::Outcome::Abstain { class, reason } => {
                    if json {
                        let _ = writeln!(
                            so,
                            "{{\"abstain\": {{\"class\": {}, \"reason\": {}}}}}",
                            json_str(&class),
                            json_str(&reason)
                        );
                    } else {
                        eprintln!("abstain: {class}: {reason}");
                        std::process::exit(3);
                    }
                }
                eval::Outcome::Fault(msg) => {
                    if json {
                        let _ = writeln!(so, "{{\"error\": {}}}", json_str(&msg));
                    } else {
                        eprintln!("als-ref: fault: {msg}");
                        std::process::exit(4);
                    }
                }
            }
        }
        _ => usage(),
    }
}
