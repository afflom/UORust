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
    /// Embed every body of a library crate's MIR as a value of the MIR datatype of the
    /// LexLean semantics, and write the project, with the census of the
    /// constructs the evaluator does not define, for `lexlean verify`.
    Embed(EmbedArgs),
    /// The layouts rustc chose for the aggregates a library's dumps print,
    /// as the dump step asks for them (`scripts/mir/dump-lib.sh`).
    Layouts(LayoutsArgs),
    /// Fetch a crate of crates.io, dump the MIR of its library, translate
    /// every body and write its register, `crates/uorust/crates/<name>`.
    Crate(CrateArgs),
    /// Merge the module files a sharded run wrote into a register, refused
    /// when a body it records as translated is residue in one, and rewrite
    /// its summary.
    Merge(MergeArgs),
    /// Verify the bridge's project, setting each theorem Lean refutes
    /// aside as pending until the rest verify, and stamp each translated
    /// row of the register's module files `bridged` or `pending`.
    Bridge(BridgeArgs),
    /// Carry a register's bridge stamps onto its census taken again on a
    /// newer head, each row's only while it is the row they were measured
    /// on.
    Carry(CarryArgs),
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
        /// Where the bundles' results, package sources and refusals are
        /// kept for `certify sync`.
        #[arg(long)]
        kept: PathBuf,
        /// The manifest of the last export: the roots it refused at an
        /// unchanged κ and compiler are refused again without a build, and
        /// the roots it certified at an unchanged identity are carried.
        #[arg(long)]
        prior: PathBuf,
        /// A directory holding the prior export's package sources as a
        /// certified crate does (`roots/<package>.rs`), searched in order
        /// (repeatable); the prior manifest's own directory when none is
        /// given.
        #[arg(long)]
        carry: Vec<PathBuf>,
        /// Seconds the export may build for: the builds still running then
        /// are stopped, and the roots not built are refused for this export.
        #[arg(long)]
        within: Option<u64>,
        /// This export is shard I (from 0) of `--shards` N: it builds every
        /// Nth root from the Ith.
        #[arg(long, requires = "shards")]
        shard: Option<usize>,
        /// The shards of a sharded export.
        #[arg(long, requires = "shard")]
        shards: Option<usize>,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Plan a sharded export: print, as JSON, the shards that build the
    /// roots the prior manifest did not refuse within `budget` seconds, and
    /// the roots whose compiler refusal the diagnose job keeps in full.
    Plan {
        /// The self register, `frontier.toml`.
        #[arg(long)]
        register: PathBuf,
        /// The manifest of the last export.
        #[arg(long)]
        prior: PathBuf,
        /// The seconds each shard may build for.
        #[arg(long)]
        budget: u64,
        /// The projection, which the roots of a cases export are read from,
        /// and with `--targets` the roots' identities, so that the plan
        /// leaves out the roots the prior export carries.
        #[arg(long)]
        projection: Option<PathBuf>,
        /// The output of `lexlean production targets`.
        #[arg(long, requires = "projection")]
        targets: Option<PathBuf>,
        /// Where the prior export's package sources are (as `certify
        /// production --carry`).
        #[arg(long)]
        carry: Vec<PathBuf>,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Build one root alone and keep everything lexlean prints of it.
    Diagnose {
        /// The self projection.
        #[arg(long)]
        projection: PathBuf,
        /// The self register, `frontier.toml`.
        #[arg(long)]
        register: PathBuf,
        /// The output of `lexlean production targets`.
        #[arg(long)]
        targets: PathBuf,
        /// The root, by its definition.
        #[arg(long)]
        root: String,
        /// The production project, with `build.log`.
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Decide, without a build, which roots of an export the compiler
    /// admits: the production project with every root marked is checked
    /// (`lexlean check`, which runs no Lean), each root it finds ineligible
    /// is refused with its diagnostic and the rest checked again. Prints the
    /// counts and each refusal, and writes `refusals.toml` in `out`.
    Roots {
        /// The projection (`uorust project <workspace> --out ...`).
        #[arg(long)]
        projection: PathBuf,
        /// The lift register of that projection.
        #[arg(long)]
        register: PathBuf,
        /// The output of `lexlean production targets`.
        #[arg(long)]
        targets: PathBuf,
        /// The directory the production project is written to.
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Take a published bundle into the certified crate and write its
    /// manifest with the bootstrap's share, under the profile the export
    /// recorded in what it kept.
    Sync {
        /// What `certify production` kept of the bundle.
        #[arg(long)]
        kept: PathBuf,
        /// The register, `frontier.toml`.
        #[arg(long)]
        register: PathBuf,
        /// The workspace's MIR dumps (`uorust mir`): the adapters' types,
        /// and the share reachable from the profile's entry.
        #[arg(long, requires = "workspace")]
        mir: Option<PathBuf>,
        /// The workspace root the dumps are of.
        #[arg(long, requires = "mir")]
        workspace: Option<PathBuf>,
        /// The certified crate's directory.
        #[arg(long)]
        dir: PathBuf,
        /// The adapters' module that is written, of a workspace that runs
        /// its certified packages in place of its functions.
        #[arg(long, requires = "mir")]
        adapters: Option<PathBuf>,
        /// The projection of the register, which the roots of a cases
        /// export are read from.
        #[arg(long)]
        projection: Option<PathBuf>,
    },
    /// The differential and the fixpoint: every output directory under `dir`
    /// written by the certified translator (`<name>-certified`) has the κ of
    /// the one the hand-written translator wrote (`<name>-hand`); prints each
    /// output's κ, and is refused on any disagreement.
    ///
    /// With `--cases`, the agreement of any workspace: every certified case
    /// taken into that directory (`certify production --cases`, `certify
    /// sync`) answers true when rustc runs it, and the test of the workspace
    /// that states it passes; prints each case with the functions it asserts
    /// on, and is refused on any case that parts.
    Agree {
        /// The directory of the outputs.
        #[arg(long, required_unless_present = "cases")]
        dir: Option<PathBuf>,
        /// The certified crate of a cases export.
        #[arg(long, conflicts_with = "dir", requires = "workspace")]
        cases: Option<PathBuf>,
        /// The workspace whose tests state the cases.
        #[arg(long, requires = "cases")]
        workspace: Option<PathBuf>,
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
        #[command(flatten)]
        profile: ProfileArgs,
    },
}

/// What an export is of, when it is not the translator's own: each option
/// given replaces what the workspace declares, and with none of them the
/// export is the translator's (uorust::mir, lift and lex, from the translate
/// entry, as `uorust_certified_` packages).
#[derive(clap::Args, Default)]
struct ProfileArgs {
    /// The workspace the export is of: every function of its register is in
    /// scope and its packages are named after its directory, unless the
    /// `[workspace.metadata.uorust.certify]` table of its Cargo manifest
    /// (`scope`, `entry`, `package`, `effects`, `target`) says otherwise.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// A crate-path prefix of the functions in scope (repeatable).
    #[arg(long)]
    scope: Vec<String>,
    /// The function the share's reachability starts at.
    #[arg(long)]
    entry: Option<String>,
    /// The prefix of every certified package's name.
    #[arg(long)]
    package: Option<String>,
    /// An effect every root admits (repeatable).
    #[arg(long)]
    effect: Vec<String>,
    /// The target every root is exported to.
    #[arg(long)]
    target: Option<String>,
    /// Export the cases: the closed assertions of the workspace's tests
    /// over the functions in scope, each a root with no parameter.
    #[arg(long)]
    cases: bool,
}

impl ProfileArgs {
    fn options(&self) -> uorust::certify::Options {
        uorust::certify::Options {
            workspace: self.workspace.clone(),
            scope: self.scope.clone(),
            entry: self.entry.clone(),
            package: self.package.clone(),
            effects: self.effect.clone(),
            target: self.target.clone(),
            cases: self.cases,
        }
    }

    fn profile(&self) -> Result<uorust::certify::Profile, UorustError> {
        uorust::certify::Profile::resolve(&self.options())
    }
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
    /// Translate only this module (repeatable), linked against every
    /// module the dumps hold: a CI shard's census, whose rows are the
    /// whole census's.
    #[arg(long)]
    only: Vec<String>,
    /// Write each module's shallow translations here, `{module}.toml`,
    /// each translated row's κ with its rendered LexLean: what `uorust
    /// embed --shallow` bridges to the evaluator.
    #[arg(long)]
    lexlean: Option<PathBuf>,
}

#[derive(clap::Args)]
struct LayoutsArgs {
    /// The crate the dumps are of: `core`, `std`.
    #[arg(long = "crate")]
    krate: String,
    /// The dumps, one directory per module; `layouts.json` is written here.
    #[arg(long)]
    mir: PathBuf,
    /// A copy of the `library` directory the dumps were built from, which
    /// the aliases are written into.
    #[arg(long)]
    copy: PathBuf,
    /// The shell command that checks the copy, rustc's diagnostics on its
    /// stderr.
    #[arg(long)]
    check: String,
}

#[derive(clap::Args)]
struct EmbedArgs {
    /// The crate the dumps are of: `core`, `std`.
    #[arg(long = "crate")]
    krate: String,
    /// A library crate to link into the program, `<crate>=<dumps>`, as
    /// `uorust crate --link` links one.
    #[arg(long = "link")]
    link: Vec<String>,
    /// The module shards: one directory of the crate's dumps per module.
    #[arg(long)]
    mir: PathBuf,
    /// The rust-src `library` directory the dumps were built from.
    #[arg(long)]
    library: PathBuf,
    /// The directory the LexLean project is written to.
    #[arg(long)]
    out: PathBuf,
    /// Embed only this module (repeatable), linked against every module
    /// the dumps hold: a CI shard's.
    #[arg(long)]
    only: Vec<String>,
    /// Write the differential check's harness crate here: a call of each
    /// sampled body's function, as rustc builds core, per case.
    #[arg(long)]
    harness: Option<PathBuf>,
    /// The harness's output: add the theorems that the evaluator returns
    /// what rustc's build returned, case by case.
    #[arg(long)]
    observed: Option<PathBuf>,
    /// A register's module files to stamp each row's embedding on, by κ:
    /// the census the merge sums is the evaluator's own.
    #[arg(long)]
    register: Option<PathBuf>,
    /// The shallow translations `uorust core --lexlean` wrote: with
    /// `--bridge`, the bridge's theorems that each refines the evaluator.
    #[arg(long, requires = "bridge")]
    shallow: Option<PathBuf>,
    /// The directory the bridge's LexLean project is written to.
    #[arg(long, requires = "shallow")]
    bridge: Option<PathBuf>,
}

#[derive(clap::Args)]
struct CarryArgs {
    /// The register's module files the stamps were measured in.
    #[arg(long)]
    from: PathBuf,
    /// The module files of the census taken again, which get them.
    #[arg(long)]
    register: PathBuf,
    /// The commit the stamps were measured at.
    #[arg(long)]
    measured: String,
}

#[derive(clap::Args)]
struct BridgeArgs {
    /// The bridge's project, as `uorust embed --bridge` wrote it.
    #[arg(long)]
    project: PathBuf,
    /// A register's module files to stamp each translated row's bridge on.
    #[arg(long)]
    register: Option<PathBuf>,
    /// When the bridge must be done, in seconds since the Unix epoch: work
    /// that could not finish by then is pending, not started.
    #[arg(long)]
    until: Option<u64>,
}

#[derive(clap::Args)]
struct CrateArgs {
    /// The crate and its version, `<name>@<version>`.
    spec: String,
    /// The directory the crates' registers are in; the register is its
    /// `<name>` directory.
    #[arg(long, default_value = "crates/uorust/crates")]
    registers: PathBuf,
    /// The rust-src `library` directory of the pinned toolchain: with it,
    /// the crate's bodies are embedded in LexLean too, and each row of its
    /// register stamped with its embedding.
    #[arg(long)]
    library: Option<PathBuf>,
    /// With `--library`, check the embedding against rustc's build of the
    /// crate: each of its functions over scalars run by both, a theorem per
    /// case that the evaluator returns what the build did, which `lexlean
    /// verify` decides; each sampled row is stamped with its theorems.
    #[arg(long, requires = "library")]
    differential: bool,
    /// A library crate to link into the crate's program, `<crate>=<dumps>`
    /// (`core=target/core-mir`, as `scripts/mir/dump-lib.sh` writes them):
    /// a call into it names its body, so a body that calls core is
    /// evaluated through core's.
    #[arg(long = "link", requires = "library")]
    link: Vec<String>,
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
        Command::Layouts(args) => {
            let n = uorust::layouts::run(&args.krate, &args.mir, &args.copy, &args.check)?;
            eprintln!("layouts: {n} types laid out");
            Ok(())
        }
        Command::Embed(args) => {
            let link = args
                .link
                .iter()
                .map(|l| {
                    l.split_once('=')
                        .map(|(k, d)| (k.to_string(), PathBuf::from(d)))
                        .ok_or_else(|| {
                            uorust::UorustError::Tool(format!("--link {l}: not <crate>=<dumps>"))
                        })
                })
                .collect::<Result<Vec<_>, UorustError>>()?;
            let link: Vec<(&str, &std::path::Path)> = link
                .iter()
                .map(|(k, d)| (k.as_str(), d.as_path()))
                .collect();
            let census = uorust::embed::embed(
                &args.krate,
                &args.library,
                &args.mir,
                &args.out,
                &args.only,
                &uorust::embed::Outputs {
                    harness: args.harness.as_deref(),
                    observed: args.observed.as_deref(),
                    register: args.register.as_deref(),
                    bridge: args.shallow.as_deref().zip(args.bridge.as_deref()),
                    differential: None,
                    link: &link,
                },
            )?;
            println!(
                "embed: {} bodies embedded, {} of them evaluable; project in {}",
                census.bodies,
                census.evaluable,
                args.out.display()
            );
            Ok(())
        }
        Command::Std(args) => library("std", &args),
        Command::Crate(args) => {
            let (name, version) = uorust::registry::parse_spec(&args.spec)?;
            let register = args.registers.join(&name);
            let lifted = uorust::registry::lift_crate(
                &name,
                &version,
                &register,
                args.library.as_deref(),
                args.differential,
                &args
                    .link
                    .iter()
                    .map(|l| {
                        l.split_once('=')
                            .map(|(k, d)| (k.to_string(), PathBuf::from(d)))
                            .ok_or_else(|| {
                                uorust::UorustError::Tool(format!(
                                    "--link {l}: not <crate>=<dumps>"
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>, UorustError>>()?,
            )?;
            println!(
                "{name}@{version}: {} of {} bodies translated in {} modules; register in {}",
                lifted.translated,
                lifted.bodies,
                lifted.modules,
                register.display()
            );
            if let Some((bodies, evaluable)) = lifted.embedding {
                println!(
                    "{name}@{version}: {bodies} bodies embedded, {evaluable} of them evaluable"
                );
            }
            if let Some((bodies, theorems)) = lifted.differential {
                println!(
                    "{name}@{version}: {theorems} differential theorems verified over {bodies} bodies"
                );
            }
            Ok(())
        }
        Command::Merge(args) => {
            // A spec naming a version is a crates.io crate, refused when its
            // name or version could name a path; only `core` and `std` style
            // names are libraries.
            let scope = match uorust::registry::parse_spec(&args.krate) {
                Ok((name, version)) => uorust::mir::core::Scope::registry(&name, &version),
                Err(error) if args.krate.contains('@') => return Err(error),
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
        Command::Bridge(args) => {
            let b = uorust::bridge::verify(
                &args.project,
                args.register.as_deref(),
                args.until
                    .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t)),
            )?;
            println!(
                "bridge: {} of {} translated bodies bridged, {} pending",
                b.bridged,
                b.bridged + b.pending,
                b.pending
            );
            Ok(())
        }
        Command::Carry(args) => {
            let (kept, dropped) =
                uorust::bridge::carry(&args.from, &args.register, &args.measured)?;
            println!(
                "carry: {kept} bridge stamps carried from {}, {dropped} rows to measure again",
                args.measured
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
    let shards = uorust::mir::core::census(&scope, root, &args.mir, &args.only)?;
    let regressed = uorust::mir::core::core_regressions(&args.register, &shards);
    if !regressed.is_empty() {
        return Err(UorustError::Tool(format!(
            "a body the {krate} register records as translated is now residue: {}",
            regressed.join("; ")
        )));
    }
    uorust::mir::core::write_register(&scope, &args.register, &shards)?;
    if let Some(dir) = &args.lexlean {
        uorust::bridge::write_shallow(dir, &shards)?;
    }
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
    uorust::register::Register::read(path, &text)
}

/// Where an export takes the prior export's package sources from: the
/// directories given, else the prior manifest's own.
fn carried_from(prior: &std::path::Path, given: Vec<PathBuf>) -> Vec<PathBuf> {
    if !given.is_empty() {
        return given;
    }
    prior
        .parent()
        .map(std::path::Path::to_path_buf)
        .into_iter()
        .collect()
}

fn certify(step: Certify) -> Result<(), UorustError> {
    use uorust::certify;
    let read = |path: &PathBuf| {
        std::fs::read_to_string(path).map_err(|e| UorustError::Io {
            path: path.clone(),
            reason: e.to_string(),
        })
    };
    match step {
        Certify::Production {
            projection,
            register,
            targets,
            out,
            kept,
            prior,
            carry,
            within,
            shard,
            shards,
            profile,
        } => {
            let manifest = certify::Manifest::read(&prior)?;
            let sources = carried_from(&prior, carry);
            let done = certify::production_build(
                &certify::Input {
                    profile: &profile.profile()?,
                    projection: &projection,
                    register: &read_register(&register)?,
                    targets: &read(&targets)?,
                },
                &out,
                &kept,
                &certify::Prior {
                    manifest: &manifest,
                    sources: &sources,
                    verify: certify::verify_project,
                },
                &certify::Compiler::installed()?,
                certify::Scope {
                    within: within.map(std::time::Duration::from_secs),
                    shard: shard.zip(shards),
                },
            )?;
            println!(
                "carried {}, built {} in {} builds, refused {}; certified bundle kept in {}",
                done.carried,
                done.built,
                done.builds,
                done.refused,
                kept.display()
            );
        }
        Certify::Sync {
            kept,
            register,
            mir,
            workspace,
            dir,
            adapters,
            projection,
        } => {
            let (profile, m) = certify::sync_paths(&certify::Sync {
                kept: &kept,
                dir: &dir,
                register: &read_register(&register)?,
                projection: projection.as_deref(),
                adapters: adapters.as_deref(),
                mir: workspace.as_deref().zip(mir.as_deref()),
            })?;
            let s = &m.share;
            match (profile.kind, &profile.entry) {
                (certify::Kind::Cases, _) => println!(
                    "{} certified roots; {} of {} cases certified, over {} functions",
                    m.roots.len(),
                    s.certified,
                    s.translator,
                    certify::cases::covered(&m).len()
                ),
                (certify::Kind::Functions, Some(entry)) => println!(
                    "{} certified roots; {} of {} translator functions certified, {} of {} reachable from {entry}",
                    m.roots.len(),
                    s.certified,
                    s.translator,
                    s.reachable_certified,
                    s.reachable
                ),
                (certify::Kind::Functions, None) => println!(
                    "{} certified roots; {} of {} functions in scope certified",
                    m.roots.len(),
                    s.certified,
                    s.translator
                ),
            }
        }
        Certify::Plan {
            register,
            prior,
            budget,
            projection,
            targets,
            carry,
            profile,
        } => {
            let profile = profile.profile()?;
            let register = read_register(&register)?;
            let roots = match (profile.kind, &projection) {
                (certify::Kind::Cases, None) => {
                    return Err(UorustError::Tool(
                        "the cases are read from the projection: pass --projection".to_string(),
                    ))
                }
                (certify::Kind::Cases, Some(projection)) => {
                    profile.export_roots(projection, &register)?
                }
                (certify::Kind::Functions, _) => profile.roots(&register),
            };
            let manifest = certify::Manifest::read(&prior)?;
            let carried = match (&projection, &targets) {
                (Some(projection), Some(targets)) => {
                    let input = certify::Input {
                        profile: &profile,
                        projection,
                        register: &register,
                        targets: &read(targets)?,
                    };
                    let sources = carried_from(&prior, carry);
                    let prior = certify::Prior {
                        manifest: &manifest,
                        sources: &sources,
                        verify: certify::verify_project,
                    };
                    let decided =
                        certify::carry(&input, &roots, &certify::Compiler::installed()?, &prior);
                    Some(decided?)
                }
                _ => None,
            };
            let plan = certify::plan_keyed(&roots, carried.as_ref(), &manifest, budget);
            let shards: Vec<String> = (0..plan.shards)
                .map(|i| format!("{i}-{}", plan.shards))
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "shards": shards,
                    "diagnose": plan.diagnose,
                    "carried": plan.carried,
                    "build": plan.build,
                    "refused": plan.refused,
                })
            );
        }
        Certify::Diagnose {
            projection,
            register,
            targets,
            root,
            out,
            profile,
        } => {
            certify::diagnose(
                &certify::Input {
                    profile: &profile.profile()?,
                    projection: &projection,
                    register: &read_register(&register)?,
                    targets: &read(&targets)?,
                },
                &out,
                &root,
            )?;
            println!("{}", out.join("build.log").display());
        }
        Certify::Roots {
            projection,
            register,
            targets,
            out,
            profile,
        } => {
            let admission = certify::admit_roots(
                &certify::Input {
                    profile: &profile.profile()?,
                    projection: &projection,
                    register: &read_register(&register)?,
                    targets: &read(&targets)?,
                },
                &out,
            )?;
            println!(
                "{} roots: {} admitted, {} refused",
                admission.roots,
                admission.admitted.len(),
                admission.refused.len()
            );
            for (cause, count) in admission.causes() {
                println!("{count}\t{cause}");
            }
        }
        Certify::Agree {
            dir,
            cases,
            workspace,
        } => match (dir, cases, workspace) {
            (_, Some(cases), Some(workspace)) => {
                let lines = certify::cases::agree(&cases, &workspace)?;
                for line in &lines {
                    println!("{line}");
                }
                println!("{} cases agree", lines.len());
            }
            (Some(dir), ..) => {
                for line in certify::agree(&dir)? {
                    println!("{line}");
                }
            }
            _ => {
                return Err(UorustError::Tool(
                    "agree needs --dir, or --cases and --workspace".to_string(),
                ))
            }
        },
        Certify::Ratchet {
            register,
            manifest,
            base_register,
            base_manifest,
            profile,
        } => {
            // An absent manifest reads as the empty one elsewhere; here it
            // would certify nothing at the base and pass every head.
            for path in [&manifest, &base_manifest] {
                if !path.is_file() {
                    return Err(UorustError::Tool(format!(
                        "certify ratchet: {} does not exist; a ratchet against an absent \
                         manifest measures nothing",
                        path.display()
                    )));
                }
            }
            // The scope the head is measured under: the options', else the
            // one its manifest states, else the translator's own.
            let stated = profile.options() != certify::Options::default();
            let head_profile = match certify::Profile::recorded_at(&manifest)? {
                Some(recorded) if !stated => recorded,
                _ => profile.profile()?,
            };
            let n = head_profile.ratchet_from(
                certify::Profile::recorded_at(&base_manifest)?.as_ref(),
                &read_register(&register)?,
                &certify::Manifest::read(&manifest)?,
                &read_register(&base_register)?,
                &certify::Manifest::read(&base_manifest)?,
            )?;
            println!(
                "{n} {} uncertified, none more than in the base",
                head_profile.measured()
            );
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
