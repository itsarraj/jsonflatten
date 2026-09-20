use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::Value;

use jsonflatten::{flat, flatten, unflatten};

#[derive(Parser)]
#[command(
    name = "jsonflatten",
    about = "Flattens nested JSON into dot-path key=value rows, and unflattens back"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Flatten a JSON file into dot-path key=value rows on stdout.
    Flatten {
        /// Path to a JSON file, or "-" for stdin.
        input: PathBuf,
    },
    /// Unflatten key=value rows (as produced by `flatten`) back into JSON.
    Unflatten {
        /// Path to a file of key=value lines, or "-" for stdin.
        input: PathBuf,
    },
}

fn read_input(path: &PathBuf) -> anyhow::Result<String> {
    if path.as_os_str() == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Ok(fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?)
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Flatten { input } => {
            let text = read_input(&input)?;
            let value: Value = serde_json::from_str(&text)
                .map_err(|e| anyhow::anyhow!("{}: invalid JSON: {e}", input.display()))?;
            for (key, val) in flatten(&value) {
                println!("{}", flat::render_line(&key, &val));
            }
        }
        Command::Unflatten { input } => {
            let text = read_input(&input)?;
            let mut map = BTreeMap::new();
            for (lineno, line) in text.lines().enumerate() {
                let line = line.trim_end();
                if line.is_empty() {
                    continue;
                }
                let (key, val) = flat::parse_line(line)
                    .map_err(|e| anyhow::anyhow!("{}:{}: {e}", input.display(), lineno + 1))?;
                map.insert(key, val);
            }
            let value = unflatten(&map);
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    }

    Ok(())
}
