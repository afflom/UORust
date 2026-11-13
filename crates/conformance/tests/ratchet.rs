//! UE-18: the hand-written translator may only shrink.
//!
//! The translator (uorust::mir, lift, lex) runs certified where `lexlean
//! production` exported its lifted definitions (crates/certified). A
//! translator function the manifest does not certify is hand-written
//! surface; a commit under which more of them are uncertified than under its
//! parent is refused, naming each new one with its cause. Both the register
//! and the manifest are written only by their generators (generate.yml's `just
//! register`, certified.yml's `just certified`), so their committed states are
//! generated states. The base is the parent commit, or HEAD when the work
//! tree changes either file (a commit in the making). There is no bypass.

use std::path::{Path, PathBuf};
use std::process::Command;

use uorust::certify::{ratchet, Manifest};
use uorust::register::Register;

const REGISTER: &str = "frontier.toml";
const MANIFEST: &str = "crates/certified/manifest.toml";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/conformance is two below the root")
        .to_path_buf()
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The base revision: HEAD when the work tree changes the register or the
/// manifest, else HEAD's first parent; none in a checkout without history.
fn base(root: &Path) -> Option<String> {
    let dirty = git(root, &["status", "--porcelain", "--", REGISTER, MANIFEST])?;
    let rev = if dirty.trim().is_empty() {
        "HEAD~1"
    } else {
        "HEAD"
    };
    git(root, &["rev-parse", "--verify", "--quiet", rev]).map(|r| r.trim().to_string())
}

fn manifest_at(root: &Path, text: Option<String>) -> Manifest {
    match text {
        Some(text) => Manifest::parse(&root.join(MANIFEST), &text).expect("the manifest parses"),
        None => Manifest::default(),
    }
}

#[test]
fn the_hand_written_translator_only_shrinks_ue_18() {
    let root = root();
    let head_text = std::fs::read_to_string(root.join(REGISTER)).expect("frontier.toml");
    let head = Register::parse(&root.join(REGISTER), &head_text).expect("the register parses");
    let head_manifest = manifest_at(&root, std::fs::read_to_string(root.join(MANIFEST)).ok());
    let Some(rev) = base(&root) else {
        // A checkout without the parent cannot be compared; CI checks out
        // the whole history (fetch-depth: 0), so this never decides there.
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must check out the parent commit to decide the ratchet"
        );
        return;
    };
    let Some(base_text) = git(&root, &["show", &format!("{rev}:{REGISTER}")]) else {
        return;
    };
    let base = Register::parse(Path::new(REGISTER), &base_text).expect("the base register parses");
    let base_manifest = manifest_at(&root, git(&root, &["show", &format!("{rev}:{MANIFEST}")]));
    if let Err(refusal) = ratchet(&head, &head_manifest, &base, &base_manifest) {
        panic!("UE-18 against {rev}: {refusal}");
    }
}

// The planted defect: the head's register with one more uncertified
// translator function than the base's is refused, the new row named with its
// cause; the base against itself passes.
#[test]
fn a_grown_translator_is_refused_ue_18() {
    let root = root();
    let text = std::fs::read_to_string(root.join(REGISTER)).expect("frontier.toml");
    let base = Register::parse(&root.join(REGISTER), &text).expect("the register parses");
    let manifest = manifest_at(&root, std::fs::read_to_string(root.join(MANIFEST)).ok());
    assert!(ratchet(&base, &manifest, &base, &manifest).is_ok());
    let planted = format!(
        "{text}\n[[residue]]\nkappa = \"{}\"\nfunction = \"uorust::mir::lower::planted\"\nreason = \"mir\"\ndetail = \"mir::Rvalue::Use\"\n",
        "0".repeat(64)
    );
    let grown =
        Register::parse(Path::new(REGISTER), &planted).expect("the planted register parses");
    let refusal =
        ratchet(&grown, &manifest, &base, &manifest).expect_err("a grown translator is refused");
    let message = refusal.to_string();
    assert!(
        message.contains("uorust::mir::lower::planted: residue (mir): mir::Rvalue::Use"),
        "{message}"
    );
}
