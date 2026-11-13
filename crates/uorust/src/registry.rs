//! A crate of crates.io as a subject of the MIR path: `uorust crate
//! <name>@<version>` fetches the crate's archive, checks it against the
//! checksum crates.io records, dumps the MIR of its library on the pinned
//! toolchain, and writes its register under `crates/uorust/crates/<name>`,
//! the same register core's and std's are: a row per body by κ in its
//! module's file, the summary in `frontier.toml`, and no κ recorded as
//! translated may come back as residue.

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
        Some((n, v)) if !n.is_empty() && !v.is_empty() => Ok((n.to_string(), v.to_string())),
        _ => Err(tool(
            "uorust crate",
            format!("`{spec}` is not <name>@<version>"),
        )),
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
            .current_dir(dir)
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
pub fn lift_crate(name: &str, version: &str, register: &Path) -> Result<Lifted, UorustError> {
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
        Ok(Lifted {
            modules: census.len(),
            bodies: census.iter().map(|s| s.rows.len()).sum(),
            translated: census.iter().map(|s| s.translated_count()).sum(),
        })
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
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

    #[test]
    fn a_spec_is_a_name_at_a_version() {
        assert_eq!(
            parse_spec("serde@1.0.0").ok(),
            Some(("serde".to_string(), "1.0.0".to_string()))
        );
        assert!(parse_spec("serde").is_err() && parse_spec("@1").is_err());
    }
}
