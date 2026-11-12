//! The route table and the router: which kind of Rust fact becomes which kind
//! of Prism record in which facet.
//!
//! `model/routes.toml` is the table's single source; this module is its Rust
//! implementation and the conformance test `routes_agree_ue_05` holds the two to
//! each other, route for route and vector for vector. Admission asks the router
//! for every fact's route; a fact the router refuses is never routed by a
//! default.

/// One route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// The route's identifier, `UR-Rnn`.
    pub id: &'static str,
    /// The kind of Rust fact the router proposes from.
    pub rust_fact: &'static str,
    /// The kind of Prism record admission emits.
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

/// Every route, in the table's order.
pub const ROUTES: &[Route] = &[
    route(
        "UR-R01",
        "crate-bin-target",
        "Foundation.Arch::Component",
        "arch",
    ),
    route(
        "UR-R02",
        "crate-lib-target",
        "Foundation.Arch::Component",
        "arch",
    ),
    route("UR-R03", "crate-doc", "Foundation.Arch::Concern", "arch"),
    route(
        "UR-R04",
        "package-author",
        "Foundation.Arch::Stakeholder",
        "arch",
    ),
    route("UR-R05", "dependency-edge", "Foundation.Arch::Edge", "arch"),
    route(
        "UR-R06",
        "clap-subcommand",
        "Foundation.Arch::Feature",
        "features",
    ),
    route("UR-R07", "pub-fn", "Feature.calls", "features"),
    route("UR-R08", "pub-trait", "Feature.calls", "features"),
    route(
        "UR-R09",
        "example-target",
        "Foundation.Arch::Journey",
        "features",
    ),
    route("UR-R10", "test-fn", "Feature.acceptance", "features"),
    route("UR-R11", "doctest", "Feature.acceptance", "features"),
    route("UR-R12", "error-enum", "LexLean.inductive", "behavior"),
    route("UR-R13", "fn-body", "LexLean.definition", "behavior"),
    route("UR-R14", "unsafe-block", "Foundation.Sec::Threat", "sec"),
    route("UR-R15", "ffi-extern", "Foundation.Sec::Threat", "sec"),
    route("UR-R16", "fs-use", "Foundation.Sec::Asset", "sec"),
    route("UR-R17", "net-use", "Foundation.Sec::Asset", "sec"),
    route("UR-R18", "process-use", "Foundation.Sec::Asset", "sec"),
    route("UR-R19", "env-use", "Foundation.Sec::Asset", "sec"),
    route(
        "UR-R20",
        "forbid-unsafe",
        "Foundation.Sec::ApplicationSecurityControl",
        "sec",
    ),
    route("UR-R21", "advisory", "Foundation.Sec::Risk", "sec"),
    route("UR-R22", "lock-package", "System.SpdxPackage", "release"),
    route(
        "UR-R23",
        "lint-config",
        "Foundation.Qual::QualityRequirement",
        "qual",
    ),
    route(
        "UR-R24",
        "rust-version",
        "Foundation.Qual::QualityRequirement",
        "qual",
    ),
    route(
        "UR-R25",
        "bench-target",
        "Foundation.Qual::QualityMeasure",
        "qual",
    ),
    route(
        "UR-R26",
        "state-enum",
        "Foundation.Arch::UmlState",
        "design",
    ),
    route(
        "UR-R27",
        "call-edge",
        "Foundation.Arch::UmlMessage",
        "design",
    ),
    route("UR-R28", "module", "Foundation.Arch::OpmThing", "design"),
];

/// The longest request the router reads, in bytes. It is derived from the
/// table, not chosen: the length of the request that names every kind once,
/// one per line. A longer request repeats a kind or names one the table does
/// not. `self_model_sm_01` holds the model's `Routes::requestLimit` to it.
pub const REQUEST_LIMIT: usize = {
    let mut bytes = 0;
    let mut at = 0;
    while at < ROUTES.len() {
        bytes += ROUTES[at].rust_fact.len() + 1;
        at += 1;
    }
    bytes - 1
};

/// What the router decides for one fact's classification tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Routing {
    /// Every tag names this route.
    Routed(Route),
    /// A tag names no route.
    Unknown(String),
    /// Two tags name different routes.
    Ambiguous(String, String),
}

/// Route a fact by its tags. The first offending tag decides a refusal.
pub fn classify(tags: &[&str]) -> Option<Routing> {
    let mut chosen: Option<(Route, &str)> = None;
    for &tag in tags {
        let Some(found) = ROUTES.iter().find(|r| r.rust_fact == tag) else {
            return Some(Routing::Unknown(tag.to_string()));
        };
        match chosen {
            None => chosen = Some((*found, tag)),
            Some((route, first)) if route.id != found.id => {
                return Some(Routing::Ambiguous(first.to_string(), tag.to_string()));
            }
            Some(_) => {}
        }
    }
    chosen.map(|(route, _)| Routing::Routed(route))
}

/// The router's wire form: one tag per line in, the decision's lines out.
pub fn respond(request: &[u8]) -> String {
    if request.is_empty() {
        return "error empty".to_string();
    }
    if request.len() > REQUEST_LIMIT {
        return "error too-large".to_string();
    }
    let Ok(text) = std::str::from_utf8(request) else {
        return "error invalid-utf8".to_string();
    };
    let mut tags: Vec<&str> = Vec::new();
    for tag in text.split('\n') {
        if tag.is_empty() {
            return "error blank-fact".to_string();
        }
        tags.push(tag);
        // The first offending line decides: a refusal among the lines so far
        // stands even if a later line is blank.
        if let Some(refusal @ (Routing::Unknown(_) | Routing::Ambiguous(..))) = classify(&tags) {
            return render(Some(refusal));
        }
    }
    render(classify(&tags))
}

fn render(routing: Option<Routing>) -> String {
    match routing {
        Some(Routing::Routed(r)) => format!(
            "route {}\nfact {}\nrecord {}\nfacet {}",
            r.id, r.rust_fact, r.prism_record, r.facet
        ),
        Some(Routing::Unknown(tag)) => format!("refused unknown-fact\nfact {tag}"),
        Some(Routing::Ambiguous(a, b)) => format!("refused ambiguous\nfact {a}\nfact {b}"),
        None => "error empty".to_string(),
    }
}
