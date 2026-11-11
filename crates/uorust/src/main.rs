//! `uorust`: project a Cargo workspace into a Prism project.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use uorust::{Options, UorustError};

/// Project any Cargo workspace into a complete, checked Prism project.
#[derive(Parser)]
#[command(name = "uorust", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// What to do with the projection.
#[derive(Subcommand)]
enum Command {
    /// Write the projection of a workspace into a directory.
    Project(Args),
    /// Check that a directory holds exactly the projection of a workspace.
    Check(Args),
    /// Print the route table.
    Routes,
}

#[derive(clap::Args)]
struct Args {
    /// The workspace root, the directory with the top `Cargo.toml`.
    workspace: PathBuf,
    /// The directory the Prism project is written to or checked in.
    #[arg(long)]
    out: PathBuf,
    /// The project name; the workspace directory's name when absent.
    #[arg(long)]
    name: Option<String>,
    /// The control set's lastModified, YYYY-MM-DDThh:mm:ssZ; the HEAD commit time when absent.
    #[arg(long)]
    last_modified: Option<String>,
    /// A RustSec advisory database checkout to check Cargo.lock against.
    #[arg(long)]
    advisories: Option<PathBuf>,
}

fn options(args: &Args) -> Result<Options, UorustError> {
    let name = match &args.name {
        Some(name) => name.clone(),
        None => uorust::default_name(&args.workspace)?,
    };
    let last_modified = match &args.last_modified {
        Some(value) => value.clone(),
        None => uorust::commit_time(&args.workspace)?,
    };
    Ok(Options {
        name,
        last_modified,
        advisories: args.advisories.clone(),
    })
}

fn run(cli: Cli) -> Result<(), UorustError> {
    match cli.command {
        Command::Project(args) => {
            let files = uorust::project_workspace(&args.workspace, &options(&args)?)?;
            uorust::write(&args.out, &files)?;
            println!("wrote {} files to {}", files.len(), args.out.display());
            Ok(())
        }
        Command::Check(args) => {
            let files = uorust::project_workspace(&args.workspace, &options(&args)?)?;
            uorust::check(&args.out, &files)?;
            println!("{} is the current projection", args.out.display());
            Ok(())
        }
        Command::Routes => {
            for route in uorust::ROUTES {
                println!(
                    "{}\t{}\t{}\t{}",
                    route.id, route.rust_fact, route.prism_record, route.facet
                );
            }
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("uorust: {error}");
            ExitCode::FAILURE
        }
    }
}
