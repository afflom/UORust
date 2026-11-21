//! A crate of crates.io as a subject of the MIR path: `uorust crate
//! <name>@<version>` fetches the crate's archive, checks it against the
//! checksum crates.io records, dumps the MIR of its library on the pinned
//! toolchain, and writes its register under `crates/uorust/crates/<name>`,
//! the same register core's and std's are: a row per body by κ in its
//! module's file, the summary in `frontier.toml`, and no κ recorded as
//! translated may come back as residue.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::mir::core::{census, core_regressions, write_register, Scope};
use crate::UorustError;

/// The module a crate's own items, outside any module, are recorded in:
/// `crate` is a keyword, so no module of the crate has the name.
pub const ROOT_MODULE: &str = "crate";

/// What `uorust crate` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lifted {
    /// The bodies embedded and those evaluable, where the crate was
    /// embedded too.
    pub embedding: Option<(usize, usize)>,
    /// The bodies the differential check ran and the theorems `lexlean`
    /// verified of them, where it was asked.
    pub differential: Option<(usize, usize)>,
    /// The modules translated.
    pub modules: usize,
    /// The bodies of the crate's MIR.
    pub bodies: usize,
    /// The bodies that translate.
    pub translated: usize,
}

fn tool(what: &str, reason: impl std::fmt::Display) -> UorustError {
    UorustError::Tool(format!("{what}: {reason}"))
}

fn run(what: &str, command: &mut Command) -> Result<Vec<u8>, UorustError> {
    let out = command.output().map_err(|e| tool(what, e))?;
    if !out.status.success() {
        return Err(tool(what, String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

/// The user agent crates.io asks automated clients to send.
const USER_AGENT: &str = "uorust-register";

/// `name@version` split, both non-empty.
pub fn parse_spec(spec: &str) -> Result<(String, String), UorustError> {
    match spec.split_once('@') {
        Some((n, v)) if !n.is_empty() && !v.is_empty() => {
            crate_component(n, v)?;
            Ok((n.to_string(), v.to_string()))
        }
        _ => Err(tool(
            "uorust crate",
            format!("`{spec}` is not <name>@<version>"),
        )),
    }
}

/// A crate's name and version as crates.io spells them, so neither can name
/// a path: the name is `[A-Za-z][A-Za-z0-9_-]{0,63}`, the version only
/// `[0-9A-Za-z.+-]` and neither `.` nor `..`.
fn crate_component(name: &str, version: &str) -> Result<(), UorustError> {
    let named = name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let versioned = !version.is_empty()
        && version != "."
        && version != ".."
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'));
    if named && versioned {
        Ok(())
    } else {
        Err(tool(
            "uorust crate",
            format!("`{name}@{version}` is not a crates.io name and version"),
        ))
    }
}

/// The item path a dump's file name names, its segments joined by `-`:
/// `serde.de-{impl#3}-f.2-0.ElaborateBoxDerefs.before.mir` names
/// `de-{impl#3}-f`.
pub fn dump_item(file: &str) -> Option<&str> {
    file.split_once('.')?.1.split('.').next()
}

/// The module a dumped item is in, given every item the crate's dumps name:
/// its first segment when that is a module's name (snake case, as a type's
/// or an impl's is not) and names no body itself (a function's closures go
/// with the function); [`ROOT_MODULE`] otherwise.
pub fn dump_module(item: &str, items: &std::collections::BTreeSet<&str>) -> String {
    match item.split_once('-') {
        Some((first, _))
            if first.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && !items.contains(first) =>
        {
            first.to_string()
        }
        _ => ROOT_MODULE.to_string(),
    }
}

/// Fetch `name@version` from crates.io into `work`, checked against the
/// checksum crates.io records, and return its unpacked directory.
fn fetch(name: &str, version: &str, work: &Path) -> Result<PathBuf, UorustError> {
    let meta = run(
        "crates.io",
        Command::new("curl").args([
            "-sSfL",
            "-A",
            USER_AGENT,
            &format!("https://crates.io/api/v1/crates/{name}/{version}"),
        ]),
    )?;
    let doc: serde_json::Value = serde_json::from_slice(&meta).map_err(|e| tool("crates.io", e))?;
    let checksum = doc
        .pointer("/version/checksum")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| tool("crates.io", format!("{name}@{version} has no checksum")))?
        .to_string();
    let archive = work.join(format!("{name}-{version}.crate"));
    run(
        "crates.io download",
        Command::new("curl").args([
            "-sSfL",
            "-A",
            USER_AGENT,
            "-o",
            &archive.display().to_string(),
            &format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
        ]),
    )?;
    let bytes = std::fs::read(&archive).map_err(|e| tool("crates.io download", e))?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != checksum {
        return Err(tool(
            "crates.io download",
            format!("{name}@{version} is {digest}, crates.io records {checksum}"),
        ));
    }
    run(
        "tar",
        Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(work),
    )?;
    let dir = work.join(format!("{name}-{version}"));
    // Its own workspace root, whatever directory it is unpacked in.
    let manifest = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| tool("Cargo.toml", e))?;
    if !text.lines().any(|l| l.trim() == "[workspace]") {
        std::fs::write(&manifest, text + "\n[workspace]\n").map_err(|e| tool("Cargo.toml", e))?;
    }
    Ok(dir)
}

/// Dump the MIR of the library of the crate at `dir` on the pinned
/// toolchain, with this machine's paths resolved away, into one directory
/// per module under `shards`.
fn dump(dir: &Path, work: &Path, shards: &Path) -> Result<(), UorustError> {
    let toolchain = format!("+{}", crate::mir::spec::TOOLCHAIN);
    let mir = work.join("mir");
    std::fs::create_dir_all(&mir).map_err(|e| tool("mir", e))?;
    // Only the library's own flags carry the dump: `cargo rustc` passes them
    // to the one target.
    run(
        "cargo rustc",
        Command::new("cargo")
            .args([toolchain.as_str(), "rustc", "--lib", "-q", "--target-dir"])
            .arg(work.join("target"))
            .arg("--")
            .arg("-Zdump-mir=ElaborateBoxDerefs")
            .arg(format!("-Zdump-mir-dir={}", mir.display()))
            .args(["-Coverflow-checks=on", "-Cdebug-assertions=off"])
            // A lint the crate denies is a newer toolchain's opinion, not
            // its build: cargo caps a registry crate's lints so when it is
            // a dependency (try-lock's `#![deny(warnings)]`).
            .arg("--cap-lints=allow")
            .current_dir(dir)
            // An inherited build directory would hold the crate built
            // already, and cargo would not run rustc to dump it again.
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("CARGO_TARGET_DIR")
            .env("RUSTC_BOOTSTRAP", "1")
            .env("CARGO_INCREMENTAL", "0"),
    )?;
    let sysroot = String::from_utf8_lossy(&run(
        "rustc --print sysroot",
        Command::new("rustc").args([toolchain.as_str(), "--print", "sysroot"]),
    )?)
    .trim()
    .to_string();
    let cargo_home = std::env::var("CARGO_HOME")
        .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()));
    let own = format!("{}/", dir.display());
    let registry = format!("{cargo_home}/registry/src/");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&mir)
        .map_err(|e| tool("mir", e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".ElaborateBoxDerefs.before.mir"))
        })
        .collect();
    files.sort();
    let names: Vec<String> = files
        .iter()
        .map(|f| {
            f.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    let items: std::collections::BTreeSet<&str> =
        names.iter().filter_map(|n| dump_item(n)).collect();
    for (file, name) in files.iter().zip(&names) {
        let Some(item) = dump_item(name) else {
            continue;
        };
        let module = dump_module(item, &items);
        let text = std::fs::read_to_string(file).map_err(|e| tool("mir", e))?;
        let text = text
            .replace(&own, "")
            .replace(&registry, "<registry>/")
            .replace(&sysroot, "<sysroot>");
        let dest = shards.join(&module);
        std::fs::create_dir_all(&dest).map_err(|e| tool("mir", e))?;
        std::fs::write(dest.join(name), text).map_err(|e| tool("mir", e))?;
    }
    Ok(())
}

/// Fetch, dump and translate `name@version` and write its register to
/// `register`, refusing it when a body the register records as translated
/// is now residue. A module file the crate no longer has is removed.
pub fn lift_crate(
    name: &str,
    version: &str,
    register: &Path,
    library: Option<&Path>,
    differential: bool,
    link: &[(String, PathBuf)],
) -> Result<Lifted, UorustError> {
    crate_component(name, version)?;
    // Under the temporary directory, named by validated components only.
    let work = std::env::temp_dir().join(format!("uorust-crate-{name}-{version}"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| tool("work", e))?;
    let result = (|| {
        let dir = fetch(name, version, &work)?;
        let shards = work.join("shards");
        std::fs::create_dir_all(&shards).map_err(|e| tool("mir", e))?;
        dump(&dir, &work, &shards)?;
        let scope = Scope::registry(name, version);
        let census = census(&scope, &dir, &shards, &[])?;
        let regressed = core_regressions(register, &census);
        if !regressed.is_empty() {
            return Err(tool(
                &format!("{name}@{version}"),
                format!(
                    "a body the register records as translated is now residue: {}",
                    regressed.join("; ")
                ),
            ));
        }
        if register.exists() {
            for entry in std::fs::read_dir(register)
                .map_err(|e| tool("register", e))?
                .filter_map(Result::ok)
            {
                let p = entry.path();
                let stem = p.file_stem().and_then(|n| n.to_str()).unwrap_or_default();
                let kept = stem == "frontier"
                    || stem == crate::queue::RETRACTED
                    || census.iter().any(|s| s.module == stem);
                if p.extension().is_some_and(|x| x == "toml") && !kept {
                    std::fs::remove_file(&p).map_err(|e| tool("register", e))?;
                }
            }
        }
        write_register(&scope, register, &census)?;
        let (embedding, checked) = match library {
            Some(library) => {
                let (e, d) = embed_crate(
                    &scope,
                    name,
                    &dir,
                    &work,
                    &shards,
                    library,
                    register,
                    differential,
                    link,
                )?;
                (Some((e.bodies, e.evaluable)), d)
            }
            None => (None, None),
        };
        Ok(Lifted {
            embedding,
            differential: checked,
            modules: census.len(),
            bodies: census.iter().map(|s| s.rows.len()).sum(),
            translated: census.iter().map(|s| s.translated_count()).sum(),
        })
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Embed the crate's bodies in LexLean with the one evaluator core's and
/// std's are, and stamp each row of its register with its embedding: the
/// crate's sources sit beside `library`'s core, alloc and std, as a crate
/// of the library does, so its declarations are read as std's are.
#[allow(clippy::too_many_arguments)]
fn embed_crate(
    scope: &Scope,
    name: &str,
    dir: &Path,
    work: &Path,
    shards: &Path,
    library: &Path,
    register: &Path,
    differential: bool,
    link: &[(String, PathBuf)],
) -> Result<(crate::embed::EmbedCensus, Option<(usize, usize)>), UorustError> {
    let link: Vec<(&str, &Path)> = link
        .iter()
        .map(|(k, d)| (k.as_str(), d.as_path()))
        .collect();
    let toolchain = format!("+{}", crate::mir::spec::TOOLCHAIN);
    let rustc = |args: &[&str]| -> Result<String, UorustError> {
        let out = run(
            "rustc",
            Command::new("rustc")
                .arg(&toolchain)
                .args(args)
                .env("RUSTC_BOOTSTRAP", "1"),
        )?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    };
    // The target the bodies were built for, as the library dumps state it.
    let host = rustc(&["-vV"])?
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| tool("rustc", "no host"))?;
    let width = rustc(&["--print", "cfg", "--target", &host])?
        .lines()
        .find_map(|l| l.strip_prefix("target_pointer_width=\""))
        .map(|w| w.trim_end_matches('"').to_string())
        .ok_or_else(|| tool("rustc", "no pointer width"))?;
    let spec: serde_json::Value = serde_json::from_str(&rustc(&[
        "-Zunstable-options",
        "--print",
        "target-spec-json",
        "--target",
        &host,
    ])?)
    .map_err(|e| tool("rustc", e))?;
    let layout = spec
        .get("data-layout")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| tool("rustc", "no data layout"))?;
    std::fs::write(
        shards.join("target.toml"),
        format!("triple = \"{host}\"\npointer_width = {width}\ndata_layout = \"{layout}\"\n"),
    )
    .map_err(|e| tool("target.toml", e))?;
    let root = work.join("root").join("library");
    std::fs::create_dir_all(&root).map_err(|e| tool("library", e))?;
    // A link's target is read from the link's own directory: each is named
    // absolutely, whatever directory `library` and the crate were given from.
    let library = std::fs::canonicalize(library).map_err(|e| tool("library", e))?;
    let dir = std::fs::canonicalize(dir).map_err(|e| tool("library", e))?;
    for krate in ["core", "alloc", "std"] {
        std::os::unix::fs::symlink(library.join(krate), root.join(krate))
            .map_err(|e| tool("library", e))?;
    }
    std::os::unix::fs::symlink(&dir, root.join(name)).map_err(|e| tool("library", e))?;
    // The crates it builds on, beside it by their names, whose enums and
    // structs its bodies make values of.
    let mut builds_on = Vec::new();
    for (lib, source) in dependencies(&dir, &toolchain)? {
        if lib != name && !root.join(&lib).exists() {
            std::os::unix::fs::symlink(&source, root.join(&lib)).map_err(|e| tool("library", e))?;
            builds_on.push(lib);
        }
    }
    layouts(name, &dir, work, shards, &library, &toolchain)?;
    let harness = work.join("harness");
    let census = crate::embed::embed_with(
        name,
        &builds_on,
        &root,
        shards,
        &work.join("embed"),
        &[],
        &crate::embed::Outputs {
            harness: differential.then_some(harness.as_path()),
            register: Some(register),
            link: &link,
            ..Default::default()
        },
    )?;
    let mut checked = None;
    if differential {
        let observed = work.join("observed.txt");
        let alone = work.join("differential");
        let theorems = if harness.join("Cargo.toml").is_file() {
            run_harness(&harness, &dir, &library, &toolchain, &observed)?;
            crate::embed::embed_with(
                name,
                &builds_on,
                &root,
                shards,
                &work.join("embed"),
                &[],
                &crate::embed::Outputs {
                    observed: Some(&observed),
                    differential: Some(&alone),
                    link: &link,
                    ..Default::default()
                },
            )?;
            verify_differential(&alone)?
        } else {
            BTreeMap::new()
        };
        stamp_differential(register, &theorems)?;
        checked = Some((theorems.len(), theorems.values().sum()));
    }
    crate::mir::core::summarize_register(scope, register)?;
    // The embedding refuses no body: every κ the register records is
    // embedded, a macro's impls of one text once.
    let missed = crate::mir::core::unembedded(register)?;
    if !missed.is_empty() {
        return Err(tool(
            &format!("{name} embedding"),
            format!(
                "{} bodies the register records are not embedded: {}",
                missed.len(),
                missed
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ));
    }
    Ok((census, checked))
}

/// The flags the crate's MIR was dumped with ([`dump`]), which the harness
/// builds the crate and the library with, so the two builds are one.
const CODEGEN: &str = "-Coverflow-checks=on -Cdebug-assertions=off";

/// Build and run the differential check's harness (`uorust embed
/// --harness`) for the crate at `dir`, writing what it prints to
/// `observed`: the crate is copied beside the harness with each wrapper
/// `wrappers.json` asks for written into its function's module, and the
/// library built from `library` with the dump's flags, as
/// scripts/mir/harness.sh builds core. A wrapper rustc refuses (a function
/// its module's file does not see, one generic past its scalars) is left
/// out, and its cases print `unbuilt`.
fn run_harness(
    harness: &Path,
    dir: &Path,
    library: &Path,
    toolchain: &str,
    observed: &Path,
) -> Result<(), UorustError> {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        sample: usize,
        module: Vec<String>,
        /// For a method, the file of its impl, from the crate's root.
        #[serde(default)]
        file: Option<String>,
        code: String,
    }
    let wrappers: Vec<Wrapper> =
        serde_json::from_str(&crate::embed::read_text(&harness.join("wrappers.json"))?)
            .map_err(|e| tool("wrappers.json", e))?;
    let copy = harness.with_file_name("harness-crate");
    copy_tree(dir, &copy)?;
    let copy = std::fs::canonicalize(&copy).map_err(|e| tool("harness", e))?;
    let home = std::fs::canonicalize(harness).map_err(|e| tool("harness", e))?;
    let lib = copy.join(crate::layouts::library_file(dir)?);
    let target = harness.join("target");
    let mut pristine: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut live: BTreeSet<usize> = wrappers.iter().map(|w| w.sample).collect();
    let cargo = |args: &[&str], flags: &str| {
        let mut c = Command::new("cargo");
        c.arg(toolchain)
            .args(args)
            .current_dir(harness)
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env("CARGO_TARGET_DIR", &target)
            .env("RUSTC_BOOTSTRAP", "1")
            .env("CARGO_INCREMENTAL", "0")
            // No debug info: what the build computes is the same, at a
            // fraction of the disk.
            .env("CARGO_PROFILE_DEV_DEBUG", "0")
            .env("RUSTFLAGS", format!("{CODEGEN} --cap-lints=allow {flags}"));
        c
    };
    loop {
        // Each live wrapper on a line of its own past the end of its
        // module's file, so an error's line names it.
        let mut at: BTreeMap<(PathBuf, usize), usize> = BTreeMap::new();
        let mut by_file: BTreeMap<PathBuf, Vec<&Wrapper>> = BTreeMap::new();
        for w in wrappers.iter().filter(|w| live.contains(&w.sample)) {
            let module: Vec<&str> = w.module.iter().map(String::as_str).collect();
            let home = match &w.file {
                Some(f) => copy.join(f),
                None => crate::layouts::module_file(&lib, &module),
            };
            by_file.entry(home).or_default().push(w);
        }
        for f in by_file.keys() {
            if !pristine.contains_key(f) {
                pristine.insert(f.clone(), crate::embed::read_text(f)?);
            }
        }
        for (f, text) in &pristine {
            let mut body = text.clone();
            if !body.ends_with('\n') {
                body.push('\n');
            }
            let mut line = body.lines().count();
            for w in by_file.get(f).into_iter().flatten() {
                line += 1;
                body.push_str(&w.code);
                body.push('\n');
                at.insert((f.clone(), line), w.sample);
            }
            std::fs::write(f, body).map_err(|e| tool("harness", e))?;
        }
        let out = cargo(&["check", "-q", "--message-format=short"], "")
            .output()
            .map_err(|e| tool("cargo check", e))?;
        if out.status.success() {
            break;
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let refused: BTreeSet<usize> = stderr
            .lines()
            .filter(|l| l.contains(": error"))
            .filter_map(|l| {
                let mut parts = l.splitn(3, ':');
                let file = parts.next()?;
                let line: usize = parts.next()?.trim().parse().ok()?;
                let file = Path::new(file);
                let file = if file.is_absolute() {
                    file.to_path_buf()
                } else {
                    home.join(file)
                };
                let file = std::fs::canonicalize(&file).unwrap_or(file);
                at.get(&(file, line)).copied()
            })
            .collect();
        if refused.is_empty() {
            // An error at no wrapper's line: the crate's copy does not
            // build beside the harness, and no wrapper is kept.
            eprintln!(
                "harness: the crate's copy does not check: {}",
                stderr.lines().take(4).collect::<Vec<_>>().join(" | ")
            );
            live.clear();
            for (f, text) in &pristine {
                std::fs::write(f, text).map_err(|e| tool("harness", e))?;
            }
            break;
        }
        eprintln!(
            "harness: rustc refused {} of {} wrappers",
            refused.len(),
            live.len()
        );
        live.retain(|k| !refused.contains(k));
    }
    // The calls of a refused wrapper's sample give way to its cases
    // printed unbuilt.
    let main = harness.join("src").join("main.rs");
    // The crate as its library is named, which its manifest may name
    // apart from its package (rustls-webpki's `webpki`).
    let pristine = crate::embed::read_text(&main)?;
    let lib = lib_name(dir)?;
    let pristine: String = pristine
        .lines()
        .map(|l| {
            match l
                .strip_prefix("use ")
                .and_then(|r| r.strip_suffix(" as _;"))
            {
                Some(_) => format!("use {lib} as _;\n"),
                None => format!("{l}\n"),
            }
        })
        .collect();
    let samples_of = sample_cases(&pristine);
    let host = String::from_utf8_lossy(&run(
        "rustc",
        Command::new("rustc").args([toolchain, "-vV"]),
    )?)
    .lines()
    .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
    .ok_or_else(|| tool("rustc", "no host"))?;
    let out = loop {
        std::fs::write(&main, select_samples(&pristine, &live)?).map_err(|e| tool("main.rs", e))?;
        let out = cargo(
            &[
                "run",
                "-q",
                "-Zbuild-std=std,panic_unwind",
                "--target",
                &host,
            ],
            "",
        )
        .env("__CARGO_TESTS_ONLY_SRC_ROOT", library)
        .output()
        .map_err(|e| tool("harness", e))?;
        if out.status.success() {
            break out.stdout;
        }
        // A case that aborts the process (a panic that does not unwind)
        // is no value to compare: its sample, the one whose case ran past
        // the last the harness printed, is left out, and the rest run
        // again.
        let stdout = String::from_utf8_lossy(&out.stdout);
        let ran = stdout
            .lines()
            .next()
            .is_some_and(|l| l.starts_with("checks "));
        let next = stdout
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(' ')?.0.parse::<usize>().ok())
            .max()
            .map_or(0, |n| n + 1);
        match samples_of.get(&next).filter(|k| ran && live.contains(k)) {
            Some(&k) => {
                eprintln!("harness: case {next} aborts the run; its sample {k} is left out");
                live.remove(&k);
            }
            None => return Err(tool("harness", String::from_utf8_lossy(&out.stderr))),
        }
    };
    std::fs::write(observed, out).map_err(|e| tool("observed", e))?;
    let _ = std::fs::remove_dir_all(&target);
    Ok(())
}

/// The name of the library of the package at `dir`: its manifest's
/// `[lib] name`, else its package's name with each `-` a `_`.
fn lib_name(dir: &Path) -> Result<String, UorustError> {
    let manifest_text = crate::embed::read_text(&dir.join("Cargo.toml"))?;
    let manifest: toml::Table =
        toml::from_str(&manifest_text).map_err(|e| tool("Cargo.toml", e))?;
    let named = manifest
        .get("lib")
        .and_then(|l| l.get("name"))
        .and_then(toml::Value::as_str);
    let package = manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| tool("Cargo.toml", "no package name"))?;
    Ok(named.unwrap_or(package).replace('-', "_"))
}

/// The sample each case of the harness's `main.rs` belongs to, by case,
/// for the samples called through a wrapper.
fn sample_cases(main: &str) -> BTreeMap<usize, usize> {
    let mut out = BTreeMap::new();
    let mut at: Option<usize> = None;
    for line in main.lines() {
        let t = line.trim();
        if let Some(k) = t.strip_prefix("// sample ") {
            at = k.parse().ok();
        } else if t.starts_with("// unbuilt ") {
            at = None;
        } else if let (Some(k), Some(rest)) = (at, t.strip_prefix("Ok(r) => println!(\"")) {
            if let Some(case) = rest.split(' ').next().and_then(|c| c.parse().ok()) {
                out.insert(case, k);
            }
        }
    }
    out
}

/// The harness's `main.rs` with the calls of each sample not `live` given
/// way to its cases printed unbuilt ([`crate::embed`]'s `harness_crate`
/// writes both, the second commented out).
fn select_samples(main: &str, live: &BTreeSet<usize>) -> Result<String, UorustError> {
    let mut kept = String::new();
    let mut skipping: Option<String> = None;
    let mut fallback = false;
    for line in main.lines() {
        let t = line.trim();
        if let Some(k) = t.strip_prefix("// sample ") {
            let k: usize = k.parse().map_err(|e| tool("main.rs", e))?;
            if !live.contains(&k) {
                skipping = Some(k.to_string());
                continue;
            }
        }
        if let Some(k) = t.strip_prefix("// unbuilt ") {
            fallback = skipping.as_deref() == Some(k);
            skipping = (!fallback).then(|| format!("end {k}"));
            continue;
        }
        if let Some(k) = t.strip_prefix("// end ") {
            fallback = false;
            if skipping.as_deref() == Some(&format!("end {k}")) {
                skipping = None;
            }
            continue;
        }
        if skipping.is_some() {
            continue;
        }
        if fallback {
            kept.push_str(&line.replacen("// ", "", 1));
        } else {
            kept.push_str(line);
        }
        kept.push('\n');
    }
    Ok(kept)
}

/// Verify the differential check's project at `dir` with `lexlean`, and
/// read from its index the theorems of each body, by κ: a theorem `lexlean`
/// refutes (the evaluator returns other than rustc's build did) fails the
/// verify, and so the crate.
fn verify_differential(dir: &Path) -> Result<BTreeMap<String, usize>, UorustError> {
    let index = dir.join("differential.toml");
    if !index.is_file() {
        return Ok(BTreeMap::new());
    }
    // Read by its lines: a case's inputs are u128 bits, past what a TOML
    // integer holds.
    let mut by_kappa: BTreeMap<String, usize> = BTreeMap::new();
    for line in crate::embed::read_text(&index)?.lines() {
        if let Some(k) = line
            .strip_prefix("kappa = \"")
            .and_then(|r| r.strip_suffix('"'))
        {
            *by_kappa.entry(k.to_string()).or_default() += 1;
        }
    }
    if by_kappa.is_empty() {
        return Ok(by_kappa);
    }
    for args in [&["lock"][..], &["verify"][..]] {
        run(
            "lexlean",
            Command::new("lexlean").args(args).current_dir(dir),
        )?;
    }
    Ok(by_kappa)
}

/// Stamp each row of the register `dir` with the differential theorems
/// verified of its body, `differential = <n>`, by κ, and clear the stamp
/// of every other row.
fn stamp_differential(dir: &Path, theorems: &BTreeMap<String, usize>) -> Result<(), UorustError> {
    for file in crate::embed::dir_paths(dir)? {
        let stem = file
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if file.extension().is_none_or(|x| x != "toml") || stem == "frontier" {
            continue;
        }
        let text = crate::embed::read_text(&file)?;
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            if line.starts_with("differential = ") {
                continue;
            }
            out.push_str(line);
            out.push('\n');
            let kappa = line
                .strip_prefix("kappa = \"")
                .and_then(|r| r.strip_suffix('"'));
            if let Some(n) = kappa.and_then(|k| theorems.get(k)) {
                out.push_str(&format!("differential = {n}\n"));
            }
        }
        if out != text {
            std::fs::write(&file, out).map_err(|e| tool("register", e))?;
        }
    }
    Ok(())
}

/// The library crates the crate at `dir` builds on, as cargo resolved them
/// for its dump: each normal dependency, and theirs, by its library's crate
/// name with the directory of its package.
fn dependencies(dir: &Path, toolchain: &str) -> Result<Vec<(String, PathBuf)>, UorustError> {
    let out = run(
        "cargo metadata",
        Command::new("cargo")
            .args([toolchain, "metadata", "-q", "--format-version", "1"])
            .current_dir(dir),
    )?;
    let meta: serde_json::Value = serde_json::from_slice(&out).map_err(|e| tool("metadata", e))?;
    let str_of =
        |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let nodes: std::collections::BTreeMap<String, &serde_json::Value> = meta
        .pointer("/resolve/nodes")
        .and_then(|n| n.as_array())
        .into_iter()
        .flatten()
        .filter_map(|n| Some((str_of(n, "id")?, n)))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut next: Vec<String> = meta
        .pointer("/resolve/root")
        .and_then(|r| r.as_str())
        .map(str::to_string)
        .into_iter()
        .collect();
    let root = next.first().cloned().unwrap_or_default();
    while let Some(id) = next.pop() {
        let Some(node) = nodes.get(&id) else { continue };
        for dep in node
            .get("deps")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
        {
            let normal = dep
                .get("dep_kinds")
                .and_then(|k| k.as_array())
                .into_iter()
                .flatten()
                .any(|k| k.get("kind").is_some_and(serde_json::Value::is_null));
            if let Some(pkg) = str_of(dep, "pkg").filter(|_| normal) {
                if seen.insert(pkg.clone()) {
                    next.push(pkg);
                }
            }
        }
    }
    let mut found = Vec::new();
    for package in meta
        .get("packages")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
    {
        let id = str_of(package, "id").unwrap_or_default();
        if id == root || !seen.contains(&id) {
            continue;
        }
        let manifest = str_of(package, "manifest_path").map(PathBuf::from);
        let lib = package
            .get("targets")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
            .find(|t| {
                t.get("kind")
                    .and_then(|k| k.as_array())
                    .is_some_and(|k| k.iter().any(|k| k == "lib" || k == "rlib"))
            })
            .and_then(|t| str_of(t, "name"));
        if let (Some(lib), Some(dir)) = (lib, manifest.as_deref().and_then(Path::parent)) {
            found.push((lib.replace('-', "_"), dir.to_path_buf()));
        }
    }
    Ok(found)
}

/// Ask rustc for the layouts of the aggregates the crate's dumps print, as
/// the library's dump step does (`crate::layouts`): in a copy of the crate
/// beside `library`'s core, alloc and std, checked by cargo with
/// `rustc_attrs` enabled for the aliases, into `shards`' layouts file.
fn layouts(
    name: &str,
    dir: &Path,
    work: &Path,
    shards: &Path,
    library: &Path,
    toolchain: &str,
) -> Result<usize, UorustError> {
    let copy = work.join("layouts").join("library");
    std::fs::create_dir_all(&copy).map_err(|e| tool("layouts", e))?;
    for krate in ["core", "alloc", "std"] {
        std::os::unix::fs::symlink(library.join(krate), copy.join(krate))
            .map_err(|e| tool("layouts", e))?;
    }
    let own = copy.join(name);
    copy_tree(dir, &own)?;
    let target = work.join("layouts").join("target");
    let quote = |p: &Path| format!("'{}'", p.display());
    let check = format!(
        "cd {} && RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR={} \
         RUSTFLAGS='-Ztrim-diagnostic-paths=no -Zcrate-attr=feature(rustc_attrs) -Coverflow-checks=on -Cdebug-assertions=off --cap-lints=allow' \
         cargo {toolchain} check -q --lib",
        quote(&own),
        quote(&target),
    );
    let n = crate::layouts::run(name, shards, &copy, &check)?;
    let _ = std::fs::remove_dir_all(&target);
    Ok(n)
}

/// Copy the tree `from` to `to`, its links followed.
fn copy_tree(from: &Path, to: &Path) -> Result<(), UorustError> {
    std::fs::create_dir_all(to).map_err(|e| tool("copy", e))?;
    for entry in std::fs::read_dir(from).map_err(|e| tool("copy", e))? {
        let entry = entry.map_err(|e| tool("copy", e))?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| tool("copy", e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dump_is_filed_under_its_module() {
        let names = [
            "serde.de-{impl#3}-f.2-0.ElaborateBoxDerefs.before.mir",
            "serde.f.2-0.ElaborateBoxDerefs.before.mir",
            "serde.f-{closure#0}.2-0.ElaborateBoxDerefs.before.mir",
            "serde.{impl#0}-g.2-0.ElaborateBoxDerefs.before.mir",
            "serde.Buffer-new.2-0.ElaborateBoxDerefs.before.mir",
        ];
        let items: std::collections::BTreeSet<&str> =
            names.iter().filter_map(|n| dump_item(n)).collect();
        let modules: Vec<String> = names
            .iter()
            .filter_map(|n| dump_item(n))
            .map(|i| dump_module(i, &items))
            .collect();
        assert_eq!(
            modules,
            ["de", ROOT_MODULE, ROOT_MODULE, ROOT_MODULE, ROOT_MODULE]
        );
    }

    /// A sample left out gives way to its cases printed unbuilt, and each
    /// case is known by its sample.
    #[test]
    fn a_sample_left_out_prints_its_cases_unbuilt() {
        let main = "fn main() {\n    // sample 0\n    match f() {\n        Ok(r) => println!(\"0 {}\", r),\n    }\n    // unbuilt 0\n    // println!(\"0 unbuilt\");\n    // end 0\n    // sample 1\n    match g() {\n        Ok(r) => println!(\"1 {}\", r),\n        Ok(r) => println!(\"2 {}\", r),\n    }\n    // unbuilt 1\n    // println!(\"1 unbuilt\");\n    // println!(\"2 unbuilt\");\n    // end 1\n}\n";
        assert_eq!(sample_cases(main), BTreeMap::from([(0, 0), (1, 1), (2, 1)]));
        let kept = select_samples(main, &BTreeSet::from([0])).unwrap();
        assert!(kept.contains("Ok(r) => println!(\"0 {}\", r)"), "{kept}");
        assert!(
            kept.contains("    println!(\"1 unbuilt\");\n    println!(\"2 unbuilt\");"),
            "{kept}"
        );
        assert!(
            !kept.contains("g()") && !kept.contains("0 unbuilt"),
            "{kept}"
        );
    }

    #[test]
    fn a_spec_is_a_name_at_a_version() {
        assert_eq!(
            parse_spec("serde@1.0.0").ok(),
            Some(("serde".to_string(), "1.0.0".to_string()))
        );
        assert!(parse_spec("serde").is_err() && parse_spec("@1").is_err());
    }

    // SEC-07: neither part of a spec can name a path.
    #[test]
    fn spec_refuses_a_path_in_the_version() {
        for spec in [
            "serde@../../x",
            "serde@1/2",
            "../x@1",
            "serde@..",
            r"serde@1\2",
        ] {
            assert!(parse_spec(spec).is_err(), "{spec}");
        }
        for spec in ["serde@1.0.219", "serde_json@1.0.0-rc.1+build"] {
            assert!(parse_spec(spec).is_ok(), "{spec}");
        }
        assert!(crate_component("serde", "..").is_err());
    }
}
