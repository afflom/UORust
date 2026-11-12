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
    /// Dump the MIR of every library and binary target of a workspace, for
    /// the MIR path of the lift (`--mir`).
    Mir(MirArgs),
    /// Decide every translation of the MIR path with `lexlean verify` and
    /// write the verdict register (`--verdicts`).
    Prove(ProveArgs),
    /// Print the route table.
    Routes,
}

#[derive(clap::Args)]
struct MirArgs {
    /// The workspace root, the directory with the top `Cargo.toml`.
    workspace: PathBuf,
    /// The directory the dumps are written to.
    #[arg(long)]
    out: PathBuf,
}

#[derive(clap::Args)]
struct ProveArgs {
    #[command(flatten)]
    args: Args,
    /// The directory the trial projections are verified in.
    #[arg(long)]
    scratch: PathBuf,
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
    /// A lift register file: the projection is admitted against it (no
    /// function it records as lifted may be decided otherwise), then
    /// `project` writes the projection's register there and `check` requires
    /// it to be current.
    #[arg(long)]
    register: Option<PathBuf>,
    /// The MIR dumps of the workspace (`uorust mir`): every function the
    /// surface lift leaves as residue is translated from its MIR.
    #[arg(long)]
    mir: Option<PathBuf>,
    /// The verdict register of the MIR path (`uorust prove`); a translation
    /// is admitted only on a proved verdict.
    #[arg(long, requires = "mir")]
    verdicts: Option<PathBuf>,
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
    let mir = match &args.mir {
        Some(dumps) => Some(uorust::mir::MirOptions {
            dumps: dumps.clone(),
            verdicts: match &args.verdicts {
                Some(path) => uorust::mir::verdict::Verdicts::read(path)?,
                None => uorust::mir::verdict::Verdicts::default(),
            },
            trial: uorust::mir::Trial::Off,
            learned: std::collections::BTreeMap::new(),
        }),
        None => None,
    };
    Ok(Options {
        name,
        last_modified,
        advisories: args.advisories.clone(),
        mir,
    })
}

fn run(cli: Cli) -> Result<(), UorustError> {
    match cli.command {
        Command::Project(args) => {
            let files = uorust::project_workspace(&args.workspace, &options(&args)?)?;
            if let Some(register) = &args.register {
                uorust::admit(register, &files)?;
            }
            uorust::write(&args.out, &files)?;
            if let Some(register) = &args.register {
                uorust::write_register(register, &files)?;
            }
            println!("wrote {} files to {}", files.len(), args.out.display());
            Ok(())
        }
        Command::Check(args) => {
            let files = uorust::project_workspace(&args.workspace, &options(&args)?)?;
            if let Some(register) = &args.register {
                uorust::check_register(register, &files)?;
            }
            uorust::check(&args.out, &files)?;
            println!("{} is the current projection", args.out.display());
            Ok(())
        }
        Command::Mir(args) => {
            uorust::mir::dump(&args.workspace, &args.out)?;
            println!(
                "dumped the MIR of {} into {}",
                args.workspace.display(),
                args.out.display()
            );
            Ok(())
        }
        Command::Prove(prove) => {
            let args = &prove.args;
            let (Some(dumps), Some(verdicts)) = (&args.mir, &args.verdicts) else {
                return Err(UorustError::Tool(
                    "prove needs --mir and --verdicts".to_string(),
                ));
            };
            let summary = uorust::mir::prove(
                &args.workspace,
                &options(args)?,
                dumps,
                verdicts,
                &prove.scratch,
            )?;
            println!(
                "proved {}, refused {} in {} verifier runs; verdicts in {}",
                summary.proved,
                summary.refused,
                summary.runs,
                verdicts.display()
            );
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
