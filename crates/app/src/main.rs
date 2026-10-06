//! See docs/specs/0001-architecture.md.

use clap::Parser;

/// Terminal Tidal player.
#[derive(Debug, Parser)]
#[command(name = "tidal-player", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
