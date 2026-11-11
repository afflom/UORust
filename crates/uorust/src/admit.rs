//! Admission: what the router proposes is admitted, or refused with a typed
//! reason that becomes an open claim. Nothing is defaulted silently.

use crate::facts::Span;

/// Why a proposal was not admitted.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Refusal {
    /// A source file does not parse, so none of its facts are known.
    Unparsable { span: Span, reason: String },
    /// A path call names more than one workspace function.
    AmbiguousCall {
        span: Span,
        callee: String,
        candidates: Vec<String>,
    },
    /// A path dependency lies outside the workspace.
    ExternalPath { package: String, dependency: String },
    /// A feature has no test, doctest or example that exercises it (PP2015).
    Unaccepted { span: Span, feature: String },
    /// Two facts project to the same LexLean name.
    Collision { span: Span, name: String },
    /// A state machine has no `#[default]` variant and no unique source state.
    NoInitialState { span: Span, machine: String },
    /// A fact carries two tags the router routes differently.
    AmbiguousFact {
        span: Span,
        fact: String,
        tags: Vec<String>,
    },
    /// A package declares no `rust-version`, so no MSRV measure exists.
    NoMsrv { package: String },
    /// No advisory database was given, so no locked package was checked.
    AdvisoriesUnconsulted,
    /// An advisory affects a locked package and nothing answers it: no
    /// advisory gate and no recorded acceptance.
    UnansweredAdvisory {
        id: String,
        package: String,
        version: String,
    },
}

impl Refusal {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Unparsable { .. } => "unparsable-source",
            Self::AmbiguousCall { .. } => "ambiguous-call",
            Self::ExternalPath { .. } => "external-path-dependency",
            Self::Unaccepted { .. } => "feature-without-acceptance",
            Self::Collision { .. } => "name-collision",
            Self::NoInitialState { .. } => "no-initial-state",
            Self::NoMsrv { .. } => "no-msrv",
            Self::AmbiguousFact { .. } => "ambiguous-fact",
            Self::AdvisoriesUnconsulted => "advisories-unconsulted",
            Self::UnansweredAdvisory { .. } => "unanswered-advisory",
        }
    }

    pub(crate) fn route(&self) -> &'static str {
        match self {
            Self::Unparsable { .. } => "UR-R28",
            Self::AmbiguousCall { .. } => "UR-R27",
            Self::ExternalPath { .. } => "UR-R05",
            Self::Unaccepted { .. } => "UR-R10",
            Self::Collision { .. } => "UR-R13",
            Self::NoInitialState { .. } | Self::AmbiguousFact { .. } => "UR-R26",
            Self::NoMsrv { .. } => "UR-R24",
            Self::AdvisoriesUnconsulted | Self::UnansweredAdvisory { .. } => "UR-R21",
        }
    }

    pub(crate) fn span(&self) -> String {
        match self {
            Self::Unparsable { span, .. }
            | Self::AmbiguousCall { span, .. }
            | Self::Unaccepted { span, .. }
            | Self::Collision { span, .. }
            | Self::NoInitialState { span, .. }
            | Self::AmbiguousFact { span, .. } => span.render(),
            Self::ExternalPath { package, .. } | Self::NoMsrv { package } => {
                format!("{package}/Cargo.toml")
            }
            Self::AdvisoriesUnconsulted | Self::UnansweredAdvisory { .. } => {
                "Cargo.lock".to_string()
            }
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Unparsable { reason, .. } => format!("the file does not parse: {reason}"),
            Self::AmbiguousCall {
                callee, candidates, ..
            } => format!(
                "the call {callee} names {} workspace functions: {}",
                candidates.len(),
                candidates.join(", ")
            ),
            Self::ExternalPath { dependency, .. } => format!(
                "dependency {dependency} is a path outside the workspace; it is neither a \
                 member nor a locked registry package"
            ),
            Self::Unaccepted { feature, .. } => format!(
                "{feature} has interface calls but no test, doctest or example exercises them"
            ),
            Self::Collision { name, .. } => {
                format!("{name} is the projection of more than one fact")
            }
            Self::NoInitialState { machine, .. } => format!(
                "{machine} has no #[default] variant and no unique state that no transition \
                 enters"
            ),
            Self::NoMsrv { .. } => "the package declares no rust-version".to_string(),
            Self::AmbiguousFact { fact, tags, .. } => format!(
                "{fact} is both {}; the router refuses it as ambiguous",
                tags.join(" and ")
            ),
            Self::AdvisoriesUnconsulted => "no RustSec advisory database was given \
                 (--advisories), so no locked package was checked against advisories"
                .to_string(),
            Self::UnansweredAdvisory {
                id,
                package,
                version,
            } => format!(
                "{id} affects {package} {version}, and the workspace has no advisory gate \
                 (deny.toml) and no recorded acceptance of it"
            ),
        }
    }
}
