//! The route table: which Rust fact becomes which Prism record in which facet.
//!
//! It is the engine's half of the agreement the self-model states in
//! `model/routes.toml`; the conformance test reads that file and requires the
//! two to name the same routes.

/// One route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// The route's identifier.
    pub id: &'static str,
    /// The Rust fact the router proposes from.
    pub rust_fact: &'static str,
    /// The Prism record admission emits.
    pub prism_record: &'static str,
    /// The facet the record belongs to.
    pub facet: &'static str,
}

const fn route(
    id: &'static str,
    rust_fact: &'static str,
    prism_record: &'static str,
    facet: &'static str,
) -> Route {
    Route {
        id,
        rust_fact,
        prism_record,
        facet,
    }
}

/// Every route the engine implements, in the order the projection applies them.
pub const ROUTES: &[Route] = &[
    route(
        "crate-target",
        "lib and bin targets",
        "Foundation.Arch::Component",
        "prism.arch",
    ),
    route(
        "external-crate",
        "direct registry dependencies",
        "Foundation.Arch::Component",
        "prism.arch",
    ),
    route(
        "dependency-edge",
        "normal dependency edges",
        "Foundation.Arch::Edge",
        "prism.arch",
    ),
    route(
        "audience",
        "lib, bin and lint presence",
        "Foundation.Arch::Stakeholder",
        "prism.arch",
    ),
    route(
        "concern",
        "description, capabilities, lints, lockfile",
        "Foundation.Arch::Concern",
        "prism.arch",
    ),
    route(
        "viewpoint",
        "stakeholder and concern pairs",
        "Foundation.Arch::Viewpoint",
        "prism.arch",
    ),
    route(
        "public-fn",
        "public functions and methods",
        "interface call",
        "features",
    ),
    route(
        "clap-subcommand",
        "clap Subcommand variants",
        "interface call",
        "features",
    ),
    route(
        "bin-entry",
        "bin main functions",
        "interface call",
        "features",
    ),
    route(
        "test",
        "tests, doctests and examples",
        "acceptance row",
        "features",
    ),
    route(
        "module-feature",
        "library modules with public functions",
        "Foundation.Arch::Feature",
        "features",
    ),
    route(
        "command-feature",
        "subcommands and bins",
        "Foundation.Arch::Feature",
        "features",
    ),
    route(
        "journey",
        "features of one crate in module order",
        "Foundation.Arch::Journey",
        "features",
    ),
    route("error-enum", "error enums", "typed failure", "behavior"),
    route(
        "pure-fn",
        "total functions over the floor",
        "LexLean definition",
        "behavior",
    ),
    route(
        "assertion",
        "closed assertions over lifted functions",
        "LexLean theorem",
        "behavior",
    ),
    route(
        "state-enum",
        "state enums and their match transitions",
        "UML state machine",
        "design",
    ),
    route(
        "call-graph",
        "resolved calls between components",
        "UML sequence and OPM views",
        "design",
    ),
    route(
        "component-view",
        "components and edges",
        "UML component view",
        "design",
    ),
    route(
        "capability",
        "fs, net, process, env, unsafe and FFI uses",
        "Foundation.Sec::Asset and Threat",
        "prism.sec",
    ),
    route(
        "control",
        "forbid(unsafe_code), lockfile, cargo-deny, reviewed sites",
        "Foundation.Sec::ApplicationSecurityControl",
        "prism.sec",
    ),
    route(
        "threat-row",
        "capabilities with STRIDE category and evidence",
        "Production.ThreatModel rows",
        "prism.sec",
    ),
    route(
        "quality",
        "doc coverage, tests, lints, MSRV, benches",
        "Foundation.Qual::QualityMeasure",
        "prism.qual",
    ),
    route(
        "lockfile",
        "Cargo.lock packages",
        "SPDX package and LockedPackage",
        "supply-chain",
    ),
];
