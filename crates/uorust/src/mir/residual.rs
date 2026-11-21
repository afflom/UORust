//! Content-bound pairs of primary and runtime MIR from a single compiler invocation.
use super::{io, parse, IrBody, UorustError, DUMPED};
use std::collections::BTreeMap;
use std::path::Path;

const RUNTIME: &str = ".runtime-optimized.after.mir";
const DIRECTORY: &str = "residue-mir";
type Pairs = BTreeMap<String, String>;

/// The compiler numbers each pass independently. Only the complete item stem
/// preceding that pass number identifies the item within one crate target.
fn item<'a>(file: &'a str, suffix: &str) -> Option<&'a str> {
    let (stem, number) = file.strip_suffix(suffix)?.rsplit_once('.')?;
    (number.split('-').count() == 3
        && number
            .split('-')
            .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())))
    .then_some(stem)
}

pub(super) fn collect(target: &Path) -> Result<(), UorustError> {
    let dir = target.join(DIRECTORY);
    let mut phases = BTreeMap::new();
    let mut early = Vec::new();
    for entry in std::fs::read_dir(target).map_err(|e| io(target, e))? {
        let path = entry.map_err(|e| io(target, e))?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(stem) = item(name, RUNTIME) {
            phases
                .entry(stem.to_owned())
                .and_modify(|slot| *slot = None)
                .or_insert(Some(path));
        } else if let Some(stem) = item(name, DUMPED) {
            early.push((stem.to_owned(), path));
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let mut index = Pairs::new();
    for (stem, path) in early {
        let Some(Some(runtime)) = phases.get(&stem) else {
            continue;
        };
        let primary = std::fs::read(&path).map_err(|e| io(&path, e))?;
        let secondary = std::fs::read(runtime).map_err(|e| io(runtime, e))?;
        let digest = crate::project::sha(&secondary);
        let destination = dir.join(&digest);
        std::fs::write(&destination, secondary).map_err(|e| io(&destination, e))?;
        index.insert(crate::project::sha(&primary), digest);
    }
    let file = dir.join("pairs.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&index).expect("string digest map"),
    )
    .map_err(|e| io(&file, e))
}

pub(super) fn index(target: &Path) -> Result<Pairs, UorustError> {
    let path = target.join(DIRECTORY).join("pairs.json");
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| UorustError::Malformed {
            path,
            reason: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Pairs::new()),
        Err(e) => Err(io(&path, e)),
    }
}

pub(super) fn read(
    target: &Path,
    index: &Pairs,
    text: &str,
    primary: &IrBody,
) -> Result<Option<IrBody>, UorustError> {
    let Some(digest) = index.get(&crate::project::sha(text.as_bytes())) else {
        return Ok(None);
    };
    let dir = target.join(DIRECTORY);
    if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(UorustError::Malformed {
            path: dir,
            reason: "invalid compiler pass digest".into(),
        });
    }
    let path = dir.join(digest);
    let bytes = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    let bad = || UorustError::Malformed {
        path: path.clone(),
        reason: "runtime MIR differs from its bound compiler body or interface".into(),
    };
    if crate::project::sha(bytes.as_bytes()) != *digest {
        return Err(bad());
    }
    // Only functions are alternative providers. The original capture still
    // owns constants, closure capture environments and compiler identities.
    if !text.lines().any(|s| s.starts_with("fn ")) || super::closure_type(primary).is_some() {
        return Ok(None);
    }
    let header = format!("// MIR for `{}`", primary.path);
    if text.lines().next() != Some(format!("{header} before ElaborateBoxDerefs").as_str()) {
        return Ok(None);
    }
    if bytes.lines().next() != Some(format!("{header} after runtime-optimized").as_str()) {
        return Err(bad());
    }
    let mut parsed = parse::bodies(&bytes).into_iter();
    let Some(body) = parsed.next() else {
        // Unsupported optimized control flow leaves the original residue open.
        return Ok(None);
    };
    if parsed.next().is_some() || body.path != primary.path {
        return Err(bad());
    }
    // Rust can specialize the printed ABI in later phases. Those captures
    // cannot discharge this particular primary interface.
    Ok((body.arity == primary.arity
        && primary.locals.get(..=primary.arity).is_some()
        && primary.locals.get(..=primary.arity) == body.locals.get(..=body.arity))
    .then_some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_recovery_binds_the_compiler_pair_and_preserves_the_interface_ue_124() {
        let root = std::env::temp_dir().join(format!("uorust-pass-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let primary = "// MIR for `f` before ElaborateBoxDerefs\n\nfn f(_1: u32) -> u32 {\n    let mut _0: u32;\n\n    bb0: {\n        _0 = copy _1;\n        return;\n    }\n}\n";
        let late = primary.replace("before ElaborateBoxDerefs", "after runtime-optimized");
        let early_path = root.join("neutral.f.2-2-009.ElaborateBoxDerefs.before.mir");
        let late_path = root.join("neutral.f.3-3-000.runtime-optimized.after.mir");
        std::fs::write(&early_path, primary).unwrap();
        std::fs::write(&late_path, &late).unwrap();
        collect(&root).unwrap();
        let entries = index(&root).unwrap();
        let original = parse::bodies(primary).remove(0);
        assert_eq!(
            read(&root, &entries, primary, &original).unwrap(),
            Some(original.clone())
        );
        assert!(
            read(&root, &entries, &(primary.to_owned() + "\n"), &original)
                .unwrap()
                .is_none()
        );
        let content = root
            .join(DIRECTORY)
            .join(crate::project::sha(late.as_bytes()));
        std::fs::write(content, "different bytes").unwrap();
        assert!(read(&root, &entries, primary, &original).is_err());
        std::fs::write(late_path, late.replace("_1: u32", "_1: u64")).unwrap();
        collect(&root).unwrap();
        assert!(read(&root, &index(&root).unwrap(), primary, &original)
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
