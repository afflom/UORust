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
    /// Which translator runs the functions the certified translator covers:
    /// the certified one, or the hand-written one, the differential's second
    /// path.
    #[arg(long, global = true, value_enum, default_value_t = Translator::Certified)]
    translator: Translator,
    #[command(subcommand)]
    command: Command,
}

/// A path of the translator.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Translator {
    /// The certified packages crates/certified holds.
    Certified,
    /// The hand-written functions they certify.
    Hand,
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
    /// Translate every body of core's MIR, as `scripts/mir/dump-core.sh`
    /// dumps it one module at a time, and write the core register.
    Core(CoreArgs),
    /// Translate every body of std's MIR, as `scripts/mir/dump-lib.sh std`
    /// dumps it one module at a time, and write the std register.
    Std(CoreArgs),
    /// Fetch a crate of crates.io, dump the MIR of its library, translate
    /// every body and write its register, `crates/uorust/crates/<name>`.
    Crate(CrateArgs),
    /// Merge the module files a sharded run wrote into a register, refused
    /// when a body it records as translated is residue in one, and rewrite
    /// its summary.
    Merge(MergeArgs),
    /// Print the route table.
    Routes,
    /// The certified translator: export the lifted translator through
    /// `lexlean production`, take the bundle in, and hold the ratchet.
    #[command(subcommand)]
    Certify(Certify),
}

/// The steps of the certified translator (`just certified`).
#[derive(Subcommand)]
enum Certify {
    /// Export the lifted translator with `lexlean production build`: the
    /// self projection with every lifted translator definition a production
    /// root, refusing each the compiler finds ineligible, and the bundle
    /// replayed with `verify-artifact`.
    Production {
        /// The self projection (`uorust project . --out ...`).
        #[arg(long)]
        projection: PathBuf,
        /// The self register, `frontier.toml`.
        #[arg(long)]
        register: PathBuf,
        /// The output of `lexlean production targets`.
        #[arg(long)]
        targets: PathBuf,
        /// The directory the production project is written to.
        #[arg(long)]
        out: PathBuf,
        /// Where the bundle's result, package sources and refusals are kept
        /// for `certify sync`.
        #[arg(long)]
        kept: PathBuf,
    },
    /// Take a published bundle into the certified crate and write its
    /// manifest with the bootstrap's share.
    Sync {
        /// What `certify production` kept of the bundle.
        #[arg(long)]
        kept: PathBuf,
        /// The self register, `frontier.toml`.
        #[arg(long)]
        register: PathBuf,
        /// The workspace's MIR dumps (`uorust mir`).
        #[arg(long)]
        mir: PathBuf,
        /// The workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// The certified crate's directory.
        #[arg(long)]
        dir: PathBuf,
        /// The adapters' module of uorust that is written.
        #[arg(long)]
        adapters: PathBuf,
    },
    /// The differential and the fixpoint: every output directory under `dir`
    /// written by the certified translator (`<name>-certified`) has the κ of
    /// the one the hand-written translator wrote (`<name>-hand`); prints each
    /// output's κ, and is refused on any disagreement.
    Agree {
        /// The directory of the outputs.
        #[arg(long)]
        dir: PathBuf,
    },
    /// Refuse a head under which more translator functions are uncertified
    /// than under the base.
    Ratchet {
        /// The head's register.
        #[arg(long)]
        register: PathBuf,
        /// The head's manifest.
        #[arg(long)]
        manifest: PathBuf,
        /// The base's register.
        #[arg(long)]
        base_register: PathBuf,
        /// The base's manifest.
        #[arg(long)]
        base_manifest: PathBuf,
    },
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
struct CoreArgs {
    /// The module shards: one directory of core's dumps per module.
    #[arg(long)]
    mir: PathBuf,
    /// The rust-src `library` directory the dumps were built from.
    #[arg(long)]
    library: PathBuf,
    /// The register directory: `frontier.toml` and one file per module.
    #[arg(long)]
    register: PathBuf,
}

#[derive(clap::Args)]
struct CrateArgs {
    /// The crate and its version, `<name>@<version>`.
    spec: String,
    /// The directory the crates' registers are in; the register is its
    /// `<name>` directory.
    #[arg(long, default_value = "crates/uorust/crates")]
    registers: PathBuf,
}

#[derive(clap::Args)]
struct MergeArgs {
    /// The crate the register is of: `core`, `std`, or `<name>@<version>`
    /// for a crate of crates.io.
    #[arg(long = "crate")]
    krate: String,
    /// The module files to merge.
    #[arg(long)]
    shards: PathBuf,
    /// The register directory.
    #[arg(long)]
    register: PathBuf,
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
    uorust::certified::use_hand(cli.translator == Translator::Hand);
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
        Command::Core(args) => library("core", &args),
        Command::Std(args) => library("std", &args),
        Command::Crate(args) => {
            let (name, version) = uorust::registry::parse_spec(&args.spec)?;
            let register = args.registers.join(&name);
            let lifted = uorust::registry::lift_crate(&name, &version, &register)?;
            println!(
                "{name}@{version}: {} of {} bodies translated in {} modules; register in {}",
                lifted.translated,
                lifted.bodies,
                lifted.modules,
                register.display()
            );
            Ok(())
        }
        Command::Merge(args) => {
            let scope = match uorust::registry::parse_spec(&args.krate) {
                Ok((name, version)) => uorust::mir::core::Scope::registry(&name, &version),
                Err(_) => uorust::mir::core::Scope::library(&args.krate, uorust::mir::TOOLCHAIN),
            };
            let merged = uorust::queue::merge(&scope, &args.register, &args.shards)?;
            println!(
                "{}: merged {} modules into {}",
                scope.krate,
                merged.len(),
                args.register.display()
            );
            Ok(())
        }
        Command::Certify(step) => certify(step),
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

/// Translate the module shards of `krate`, a crate of the pinned
/// toolchain's rust-src, and write its register.
fn library(krate: &str, args: &CoreArgs) -> Result<(), UorustError> {
    let scope = uorust::mir::core::Scope::library(krate, uorust::mir::TOOLCHAIN);
    // The dumps name sources `library/<crate>/src/..`.
    let root = args.library.parent().unwrap_or(&args.library);
    let shards = uorust::mir::core::census(&scope, root, &args.mir)?;
    let regressed = uorust::mir::core::core_regressions(&args.register, &shards);
    if !regressed.is_empty() {
        return Err(UorustError::Tool(format!(
            "a body the {krate} register records as translated is now residue: {}",
            regressed.join("; ")
        )));
    }
    uorust::mir::core::write_register(&scope, &args.register, &shards)?;
    let (all, done) = shards.iter().fold((0, 0), |(a, d), s| {
        (a + s.rows.len(), d + s.translated_count())
    });
    println!(
        "{krate}: {done} of {all} bodies translated in {} modules; register in {}",
        shards.len(),
        args.register.display()
    );
    Ok(())
}

fn read_register(path: &std::path::Path) -> Result<uorust::register::Register, UorustError> {
    let text = std::fs::read_to_string(path).map_err(|e| UorustError::Io {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    uorust::register::Register::parse(path, &text)
}

fn certify(step: Certify) -> Result<(), UorustError> {
    use uorust::certify;
    match step {
        Certify::Production {
            projection,
            register,
            targets,
            out,
            kept,
        } => {
            let targets = std::fs::read_to_string(&targets).map_err(|e| UorustError::Io {
                path: targets.clone(),
                reason: e.to_string(),
            })?;
            certify::production_build(
                &projection,
                &out,
                &read_register(&register)?,
                &targets,
                &kept,
            )?;
            println!("certified bundle kept in {}", kept.display());
        }
        Certify::Sync {
            kept,
            register,
            mir,
            workspace,
            dir,
            adapters,
        } => {
            let m = certify::sync_paths(
                &kept,
                &dir,
                &adapters,
                &read_register(&register)?,
                &workspace,
                &mir,
            )?;
            let s = &m.share;
            println!(
                "{} certified roots; {} of {} translator functions certified, {} of {} reachable from {}",
                m.roots.len(),
                s.certified,
                s.translator,
                s.reachable_certified,
                s.reachable,
                certify::ENTRY
            );
        }
        Certify::Agree { dir } => {
            for line in certify::agree(&dir)? {
                println!("{line}");
            }
        }
        Certify::Ratchet {
            register,
            manifest,
            base_register,
            base_manifest,
        } => {
            let n = certify::ratchet(
                &read_register(&register)?,
                &certify::Manifest::read(&manifest)?,
                &read_register(&base_register)?,
                &certify::Manifest::read(&base_manifest)?,
            )?;
            println!("{n} translator functions uncertified, none more than in the base");
        }
    }
    Ok(())
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
