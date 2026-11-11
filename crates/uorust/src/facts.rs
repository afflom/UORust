//! Facts read from a workspace, each with the source span it came from.

use std::collections::BTreeMap;

/// A place in the workspace: a path relative to the workspace root and a line.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Span {
    pub(crate) file: String,
    pub(crate) line: usize,
}

impl Span {
    pub(crate) fn render(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    /// The workspace directory name, the project's name.
    pub(crate) name: String,
    pub(crate) packages: Vec<Package>,
    pub(crate) lock: Vec<LockedPackage>,
    pub(crate) has_deny: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct LockedPackage {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: String,
    pub(crate) checksum: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) license: String,
    pub(crate) rust_version: Option<String>,
    pub(crate) authors: Vec<String>,
    /// The package directory relative to the workspace root.
    pub(crate) dir: String,
    pub(crate) targets: Vec<Target>,
    pub(crate) deps: Vec<Dep>,
    /// Lint levels the package declares or inherits.
    pub(crate) lints: BTreeMap<String, String>,
    pub(crate) files: Vec<SourceFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TargetKind {
    Lib,
    Bin,
    Test,
    Example,
    Bench,
}

#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) name: String,
    pub(crate) kind: TargetKind,
    /// The root source file relative to the workspace root.
    pub(crate) src: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Dep {
    pub(crate) name: String,
    /// `normal`, `dev` or `build`.
    pub(crate) kind: String,
    /// The path of a path dependency relative to the workspace root, or the
    /// absolute path when it lies outside it.
    pub(crate) path: Option<String>,
    pub(crate) inside: bool,
}

/// What one source file holds.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceFile {
    pub(crate) rel: String,
    /// The target this file belongs to: its root's index in `Package::targets`.
    pub(crate) target: usize,
    /// The module path inside the target, `""` for the root.
    pub(crate) module: String,
    pub(crate) parse_error: Option<String>,
    pub(crate) doc: String,
    pub(crate) forbids_unsafe: bool,
    pub(crate) fns: Vec<FnFact>,
    pub(crate) enums: Vec<EnumFact>,
    pub(crate) error_impls: Vec<String>,
    pub(crate) capabilities: Vec<(Capability, Span, String)>,
    /// `use` imports: the imported name -> the path it names.
    pub(crate) imports: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct FnFact {
    pub(crate) name: String,
    /// `Type` for a method of `impl Type`.
    pub(crate) owner: Option<String>,
    pub(crate) public: bool,
    pub(crate) test: bool,
    pub(crate) in_test_module: bool,
    pub(crate) span: Span,
    pub(crate) doc: String,
    pub(crate) doctests: usize,
    pub(crate) signature: String,
    /// Path calls, `a::b::c` as written.
    pub(crate) calls: Vec<String>,
    /// String literals in the body.
    pub(crate) literals: Vec<String>,
    /// `assert!`/`assert_eq!` arguments, as source text.
    pub(crate) asserts: Vec<(String, Vec<syn::Expr>)>,
    /// Arms `E::A => E::B` of a match in the body.
    pub(crate) transitions: Vec<(String, String, String)>,
    pub(crate) item: Option<syn::Signature>,
    pub(crate) block: Option<syn::Block>,
}

#[derive(Clone, Debug)]
pub(crate) struct EnumFact {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) derives: Vec<String>,
    /// Variant name, whether it is a unit variant, its doc, whether `#[default]`.
    pub(crate) variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub(crate) struct Variant {
    pub(crate) name: String,
    pub(crate) unit: bool,
    pub(crate) doc: String,
    pub(crate) default: bool,
}

/// An effect a target reaches beyond pure computation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Capability {
    Fs,
    Net,
    Process,
    Env,
    Unsafe,
    Ffi,
}

impl Capability {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Fs => "Fs",
            Self::Net => "Net",
            Self::Process => "Process",
            Self::Env => "Env",
            Self::Unsafe => "Unsafe",
            Self::Ffi => "Ffi",
        }
    }
}
