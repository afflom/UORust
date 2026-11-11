"""Emit UORust's facet modules (Architecture, Design, Features, Security, Quality)
with every index equal to the record's position in identifier order."""
import sys

root = sys.argv[1]
OUT = f"{root}/app/src"

def q(s):
    assert "\n" not in s
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'

def defn(name, result, body):
    return f"definition{{\n  name: {name},\n  parameters: [],\n  result: {result},\n  body: {body}\n}}\n"

def strlist(items):
    out = "(nil String)"
    for i in reversed(items):
        out = f"(cons {q(i)} {out})"
    return out

def header(module, title, imports, glossaries=("prism.arch",)):
    g = "\\useglossary{lexlean.std.bool@1.2.0}\n\\useglossary{lexlean.std.nat@1.2.0}\n"
    for gl in glossaries:
        g += f"\\useglossary{{{gl}@1.0.0}}\n"
    im = "".join(f"\\importmodule{{{i}}}\n" for i in imports)
    return f"\\begin{{lexlean}}{{{module}}}\n{g}{im}\\title{{{title}}}\n\\begin{{semanticmodule}}\n"

FOOT = "\\end{semanticmodule}\n\\end{lexlean}\n"

def indexed(names):
    """position of each declaration name in identifier byte order"""
    return {n: i for i, n in enumerate(sorted(names, key=lambda n: n.encode()))}

# ---------------------------------------------------------------- Architecture
A = "Architecture"
stakeholders = [
    ("stakeholderRustDeveloper", "Rust developer", "Owns a Cargo workspace and runs uorust to get a Prism model of it without writing the model by hand."),
    ("stakeholderPrismModeler", "Prism modeler", "Reads, checks and extends the Prism project uorust emits, and relies on every record tracing to a source span."),
    ("stakeholderSecurityReviewer", "Security reviewer", "Reviews the threat model uorust derives from unsafe code, effects and the supply chain, and the controls that answer it."),
    ("stakeholderDevRelAuthor", "DevRel author", "Writes the walkthroughs and explorers that teach UORust, and covers every facet record with a resource."),
    ("stakeholderMaintainer", "Maintainer", "Changes the route table, the engine and this model, and keeps the Rust router, the verified table and the Pages site in step."),
]
concerns = [
    ("concernCompleteness", "Completeness of the model", "Every fact uorust reads is routed to exactly one record kind or refused; nothing is dropped silently."),
    ("concernFidelity", "Fidelity to the source", "Every emitted record traces to the Rust item, manifest entry or lock entry it came from."),
    ("concernDeterminism", "Deterministic output", "Running uorust twice on the same workspace writes byte-identical files."),
    ("concernNoDefaults", "No silent defaults", "An ambiguous or unknown fact is refused with a typed reason and recorded as an open claim, never resolved by a guess."),
    ("concernSupplyChain", "Supply-chain integrity", "The released binary, its dependencies and the published page are what the locked sources say they are."),
    ("concernReadOnly", "Read-only ingestion", "uorust reads the subject workspace and never builds it, runs its build scripts or sends its source anywhere."),
    ("concernDiscoverability", "Discoverability", "A newcomer can learn what UORust does from its published page and run its route table in the browser."),
]
components = [
    ("componentCli", "releaseBinary", "uorust CLI", "The uorust release binary: project, check and routes subcommands over a Cargo workspace."),
    ("componentIngest", "service", "Ingest", "Reads cargo metadata, Cargo.lock and every source file through syn into facts carrying their source spans."),
    ("componentRouter", "service", "Router", "Maps each fact's kinds to the one Prism record kind of model/routes.toml, or to a typed refusal."),
    ("componentAdmission", "service", "Admission", "Admits routed facts whose records are complete and records each refusal as an open claim with its reason."),
    ("componentEmitter", "service", "Emitter", "Writes the Prism project: prismpm.toml, lexlean.toml, one module per facet, glossaries and the conformance register, in a fixed order."),
    ("componentRouteGuest", "service", "Route-table guest", "The Core-Wasm guest compiled from app/src/Routes.lex.tex: the verified route table, replayed against every acceptance vector."),
    ("componentRouteCli", "cliAdapter", "Route CLI adapter", "The generated uorust-routes program that answers the same route requests as the page from a file or standard input."),
    ("componentPagesSite", "staticSite", "Pages site", "The GitHub Pages site that describes UORust and runs the route-table guest in the browser."),
    ("componentSubjectWorkspace", "externalSystem", "Subject workspace", "The Cargo workspace uorust reads: manifests, lock file and Rust sources."),
    ("componentCargo", "externalSystem", "Cargo", "The cargo metadata command that resolves the subject workspace's packages, targets and dependencies."),
    ("componentCratesIo", "externalSystem", "crates.io", "The registry the locked dependencies of uorust and of the subject come from."),
    ("componentRustc", "externalSystem", "rustc toolchain", "The pinned Rust toolchain that compiles uorust with forbid(unsafe_code) and the workspace lints."),
    ("componentPrismPm", "externalSystem", "PrismPM SDK", "The locked PrismPM and LexLean SDK that checks the emitted project and verifies this model."),
    ("componentGitHubActions", "externalSystem", "GitHub Actions", "The CI that runs just vv on every push and deploys the Pages site."),
]
edges = [
    ("edgeWorkspaceIngest", "componentSubjectWorkspace", "componentIngest", "dataFlow"),
    ("edgeCargoIngest", "componentCargo", "componentIngest", "dataFlow"),
    ("edgeWorkspaceCargo", "componentSubjectWorkspace", "componentCargo", "dataFlow"),
    ("edgeCratesIoCargo", "componentCratesIo", "componentCargo", "dataFlow"),
    ("edgeCliIngest", "componentCli", "componentIngest", "controlFlow"),
    ("edgeIngestRouter", "componentIngest", "componentRouter", "dataFlow"),
    ("edgeRouterAdmission", "componentRouter", "componentAdmission", "dataFlow"),
    ("edgeAdmissionEmitter", "componentAdmission", "componentEmitter", "dataFlow"),
    ("edgeEmitterCli", "componentEmitter", "componentCli", "dataFlow"),
    ("edgeEmitterPrismPm", "componentEmitter", "componentPrismPm", "dataFlow"),
    ("edgeRustcCli", "componentRustc", "componentCli", "controlFlow"),
    ("edgeCratesIoCli", "componentCratesIo", "componentCli", "dataFlow"),
    ("edgeRouterGuest", "componentRouter", "componentRouteGuest", "controlFlow"),
    ("edgeRouteCliGuest", "componentRouteCli", "componentRouteGuest", "dataFlow"),
    ("edgeGuestRouteCli", "componentRouteGuest", "componentRouteCli", "dataFlow"),
    ("edgePagesGuest", "componentPagesSite", "componentRouteGuest", "dataFlow"),
    ("edgeGuestPages", "componentRouteGuest", "componentPagesSite", "dataFlow"),
    ("edgePrismPmGuest", "componentPrismPm", "componentRouteGuest", "controlFlow"),
    ("edgeActionsPages", "componentGitHubActions", "componentPagesSite", "controlFlow"),
    ("edgeActionsCli", "componentGitHubActions", "componentCli", "controlFlow"),
]
MK = {"componentModel": 0, "qualityModel": 1, "securityModel": 2}
vp_text = {
    "viewpointPipelineCompleteness": ("Pipeline for completeness", "Where each fact is routed, admitted or refused, so none is dropped."),
    "viewpointPipelineFidelity": ("Pipeline for fidelity", "Which component carries a fact's source span to the record it becomes."),
    "viewpointRefusal": ("Admission for refusal", "How an ambiguous or unknown fact becomes an open claim instead of a record."),
    "viewpointDeterminism": ("Emission for determinism", "What fixes the order and bytes of the emitted files."),
    "viewpointSupplyChain": ("Supply chain", "Where the crates, the toolchain and the released binary come from."),
    "viewpointReadOnly": ("Ingestion boundary", "What uorust reads from the subject workspace and what it never does with it."),
    "viewpointDiscoverability": ("Published page", "What the Pages site shows and runs."),
}
viewpoints = [
    # name, stakeholder, concern, modelKind
    ("viewpointPipelineCompleteness", "stakeholderPrismModeler", "concernCompleteness", "componentModel"),
    ("viewpointPipelineFidelity", "stakeholderRustDeveloper", "concernFidelity", "componentModel"),
    ("viewpointRefusal", "stakeholderPrismModeler", "concernNoDefaults", "componentModel"),
    ("viewpointDeterminism", "stakeholderMaintainer", "concernDeterminism", "qualityModel"),
    ("viewpointSupplyChain", "stakeholderSecurityReviewer", "concernSupplyChain", "securityModel"),
    ("viewpointReadOnly", "stakeholderSecurityReviewer", "concernReadOnly", "securityModel"),
    ("viewpointDiscoverability", "stakeholderDevRelAuthor", "concernDiscoverability", "componentModel"),
]
views = [(vp.replace("viewpoint", "view", 1), vp) for vp, *_ in viewpoints]

si = indexed([n for n, *_ in stakeholders]); ci = indexed([n for n, *_ in concerns])
pi = indexed([n for n, *_ in components]); ei = indexed([n for n, *_ in edges])
vpi = indexed([n for n, *_ in viewpoints]); vi = indexed([n for n, _ in views])

out = header(A, "UORust architecture description", ["Foundation.Arch"])
out += defn("standardIso42010", "Foundation.Arch::StandardReference",
            '(record Foundation.Arch::StandardReference {standardIndex := 4, editionIndex := 0, standardId := "ISO-42010-2022", edition := "2022"})') + "\n"
for n, name, text in stakeholders:
    out += defn(n, "Foundation.Arch::Stakeholder", f"(record Foundation.Arch::Stakeholder {{index := {si[n]}, name := {q(name)}, text := {q(text)}}})") + "\n"
for n, name, text in concerns:
    out += defn(n, "Foundation.Arch::Concern", f"(record Foundation.Arch::Concern {{index := {ci[n]}, name := {q(name)}, text := {q(text)}}})") + "\n"
for n, kind, name, text in components:
    out += defn(n, "Foundation.Arch::Component", f"(record Foundation.Arch::Component {{index := {pi[n]}, kind := (ctor Foundation.Arch::ComponentKind.{kind}), name := {q(name)}, text := {q(text)}}})") + "\n"
for n, a, b, kind in edges:
    out += defn(n, "Foundation.Arch::Edge", f"(record Foundation.Arch::Edge {{index := {ei[n]}, fromIndex := {pi[a]}, toIndex := {pi[b]}, kind := (ctor Foundation.Arch::EdgeKind.{kind})}})") + "\n"
for n, s, c, mk in viewpoints:
    out += defn(n, "Foundation.Arch::Viewpoint", f"(record Foundation.Arch::Viewpoint {{index := {vpi[n]}, stakeholderIndex := {si[s]}, concernIndex := {ci[c]}, modelKindIndex := {MK[mk]}}})") + "\n"
vpmk = {n: MK[mk] for n, s, c, mk in viewpoints}
for n, vp in views:
    out += defn(n, "Foundation.Arch::View", f"(record Foundation.Arch::View {{index := {vi[n]}, viewpointIndex := {vpi[vp]}, modelKindIndex := {vpmk[vp]}}})") + "\n"
out += """structure{
  name: Narrative,
  type_parameters: [],
  parameters: [],
  fields: [{name: record, type: String}, {name: name, type: String}, {name: text, type: String}]
}

"""
nl = "(nil Narrative)"
for n, *_ in reversed(viewpoints):
    t, x = vp_text[n]
    nl = f"(cons (record Narrative {{record := {q('Architecture::' + n)}, name := {q(t)}, text := {q(x)}}}) {nl})"
out += defn("viewpointNarratives", "(List Narrative)", nl) + "\n"
def lst(names, ty):
    o = f"(nil {ty})"
    for n in reversed(names):
        o = f"(cons ({n}) {o})"
    return o
out += defn("edgeRecords", "(List Foundation.Arch::Edge)", lst([e[0] for e in edges], "Foundation.Arch::Edge")) + "\n"
out += defn("viewpointRecords", "(List Foundation.Arch::Viewpoint)", lst([v[0] for v in viewpoints], "Foundation.Arch::Viewpoint")) + "\n"
out += """definition{
  name: edgesWithin,
  parameters: [items : (List Foundation.Arch::Edge), count : Nat],
  result: Bool,
  body: (match items
    | List.nil => true
    | List.cons head rest => (((project head fromIndex) <? count) && (((project head toIndex) <? count) && ((not ((project head fromIndex) =? (project head toIndex))) && (edgesWithin rest count))))),
  recursive_argument: items
}

definition{
  name: viewpointsFrame,
  parameters: [items : (List Foundation.Arch::Viewpoint), stakeholders : Nat, concerns : Nat],
  result: Bool,
  body: (match items
    | List.nil => true
    | List.cons head rest => (((project head stakeholderIndex) <? stakeholders) && (((project head concernIndex) <? concerns) && (((project head modelKindIndex) <? 3) && (viewpointsFrame rest stakeholders concerns))))),
  recursive_argument: items
}

"""
out += f"""theorem{{
  name: edgesJoinDistinctComponents,
  parameters: [],
  statement: ((edgesWithin (edgeRecords) {len(components)}) = true),
  proof: decide{{}}
}}

theorem{{
  name: viewpointsFrameDeclaredConcerns,
  parameters: [],
  statement: ((viewpointsFrame (viewpointRecords) {len(stakeholders)} {len(concerns)}) = true),
  proof: decide{{}}
}}
"""
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Architecture.lex.tex", "w").write(out)

# ---------------------------------------------------------------- Design
D = "Design"
def C(n): return f"Architecture::{n}"
def Dn(n): return f"Design::{n}"
processes = [
    ("processRun", "componentCli"), ("processIngest", "componentIngest"), ("processRoute", "componentRouter"),
    ("processAdmit", "componentAdmission"), ("processRefuse", "componentAdmission"), ("processRecordClaim", "componentAdmission"),
    ("processRelease", "componentAdmission"), ("processEmit", "componentEmitter"), ("processReplayVectors", "componentRouteGuest"),
]
states = [
    ("stateFactProposed", "componentAdmission"), ("stateFactAdmitted", "componentAdmission"), ("stateFactRefused", "componentAdmission"),
    ("stateFactOpenClaim", "componentAdmission"), ("stateFactReleased", "componentAdmission"),
]
dviews = [
    ("viewOpmPipeline", "The projection process", "viewpointPipelineCompleteness", "opmObjectProcess"),
    ("viewUmlComponents", "Components of the pipeline", "viewpointPipelineFidelity", "umlComponent"),
    ("viewUmlSequence", "A workspace through ingest, route, admit and emit", "viewpointPipelineFidelity", "umlSequence"),
    ("viewUmlFactAdmission", "States of a fact's admission", "viewpointRefusal", "umlStateMachine"),
]
out = header(D, "UORust design views", ["Architecture", "Foundation.Arch"])
for n, owner in processes:
    out += defn(n, "Foundation.Arch::SystemProcess", f"(record Foundation.Arch::SystemProcess {{owner := {q(C(owner))}}})") + "\n"
for n, owner in states:
    out += defn(n, "Foundation.Arch::SystemState", f"(record Foundation.Arch::SystemState {{owner := {q(C(owner))}}})") + "\n"
for n, title, vp, diag in dviews:
    out += defn(n, "Foundation.Arch::DesignView", f"(record Foundation.Arch::DesignView {{title := {q(title)}, viewpoint := {q(C(vp))}, diagram := (ctor Foundation.Arch::DesignDiagram.{diag})}})") + "\n"
V = Dn("viewOpmPipeline")
opm_things = [("thingCli", "object", C("componentCli")), ("thingWorkspace", "object", C("componentSubjectWorkspace")),
              ("thingRouter", "object", C("componentRouter")), ("thingAdmission", "object", C("componentAdmission")),
              ("thingPrismPm", "object", C("componentPrismPm")),
              ("thingRun", "process", Dn("processRun")), ("thingIngest", "process", Dn("processIngest")),
              ("thingRoute", "process", Dn("processRoute")), ("thingAdmit", "process", Dn("processAdmit")),
              ("thingEmit", "process", Dn("processEmit"))]
for n, kind, subj in opm_things:
    out += defn(n, "Foundation.Arch::OpmThing", f"(record Foundation.Arch::OpmThing {{view := {q(V)}, kind := (ctor Foundation.Arch::OpmThingKind.{kind}), subject := {q(subj)}}})") + "\n"
for n, st in [("opmStateProposed", "stateFactProposed"), ("opmStateAdmitted", "stateFactAdmitted"), ("opmStateRefused", "stateFactRefused")]:
    out += defn(n, "Foundation.Arch::OpmState", f"(record Foundation.Arch::OpmState {{view := {q(V)}, thing := {q(C('componentAdmission'))}, state := {q(Dn(st))}}})") + "\n"
links = [
    ("linkCliRuns", "agent", C("componentCli"), Dn("processRun")),
    ("linkWorkspaceIngested", "instrument", C("componentSubjectWorkspace"), Dn("processIngest")),
    ("linkRouterRoutes", "instrument", C("componentRouter"), Dn("processRoute")),
    ("linkAdmissionChanged", "effect", C("componentAdmission"), Dn("processAdmit")),
    ("linkEmitResult", "result", Dn("processEmit"), C("componentPrismPm")),
    ("linkIngestInvokesRoute", "invocation", Dn("processIngest"), Dn("processRoute")),
    ("linkRouteInvokesAdmit", "invocation", Dn("processRoute"), Dn("processAdmit")),
    ("linkAdmitInvokesEmit", "invocation", Dn("processAdmit"), Dn("processEmit")),
]
for n, kind, a, b in links:
    out += defn(n, "Foundation.Arch::OpmLink", f"(record Foundation.Arch::OpmLink {{view := {q(V)}, kind := (ctor Foundation.Arch::OpmLinkKind.{kind}), source := {q(a)}, target := {q(b)}}})") + "\n"
for i, sub in enumerate(["processIngest", "processRoute", "processAdmit", "processEmit"]):
    out += defn(f"zoomRun{i}", "Foundation.Arch::OpmZoom", f"(record Foundation.Arch::OpmZoom {{view := {q(V)}, process := {q(Dn('processRun'))}, subprocess := {q(Dn(sub))}, order := {i}}})") + "\n"
V = Dn("viewUmlComponents")
ucomps = ["componentCli", "componentIngest", "componentRouter", "componentAdmission", "componentEmitter"]
for c in ucomps:
    out += defn("uml" + c[0].upper() + c[1:], "Foundation.Arch::UmlComponent", f"(record Foundation.Arch::UmlComponent {{view := {q(V)}, component := {q(C(c))}}})") + "\n"
for e in ["edgeCliIngest", "edgeIngestRouter", "edgeRouterAdmission", "edgeAdmissionEmitter", "edgeEmitterCli"]:
    out += defn("connector" + e[4:], "Foundation.Arch::UmlConnector", f"(record Foundation.Arch::UmlConnector {{view := {q(V)}, edge := {q(C(e))}}})") + "\n"
V = Dn("viewUmlSequence")
for i, c in enumerate(ucomps):
    out += defn("lifeline" + c[9:], "Foundation.Arch::UmlLifeline", f"(record Foundation.Arch::UmlLifeline {{view := {q(V)}, component := {q(C(c))}, order := {i}}})") + "\n"
msgs = [
    ("synchronous", "componentCli", "componentIngest", "processIngest"),
    ("synchronous", "componentIngest", "componentRouter", "processRoute"),
    ("synchronous", "componentRouter", "componentAdmission", "processAdmit"),
    ("synchronous", "componentAdmission", "componentAdmission", "processRefuse"),
    ("synchronous", "componentAdmission", "componentEmitter", "processEmit"),
    ("reply", "componentEmitter", "componentAdmission", "processEmit"),
]
for i, (kind, a, b, pr) in enumerate(msgs):
    out += defn(f"message{i}", "Foundation.Arch::UmlMessage", f"(record Foundation.Arch::UmlMessage {{view := {q(V)}, kind := (ctor Foundation.Arch::UmlMessageKind.{kind}), order := {i}, source := {q(C(a))}, target := {q(C(b))}, process := {q(Dn(pr))}}})") + "\n"
V = Dn("viewUmlFactAdmission")
for n, _ in states:
    out += defn("uml" + n[0].upper() + n[1:], "Foundation.Arch::UmlState", f"(record Foundation.Arch::UmlState {{view := {q(V)}, state := {q(Dn(n))}, initial := {'true' if n == 'stateFactProposed' else 'false'}}})") + "\n"
trans = [("stateFactProposed", "stateFactAdmitted", "processAdmit"), ("stateFactProposed", "stateFactRefused", "processRefuse"),
         ("stateFactRefused", "stateFactOpenClaim", "processRecordClaim"), ("stateFactAdmitted", "stateFactReleased", "processRelease")]
for i, (a, b, t) in enumerate(trans):
    out += defn(f"transition{i}", "Foundation.Arch::UmlTransition", f"(record Foundation.Arch::UmlTransition {{view := {q(V)}, order := {i}, source := {q(Dn(a))}, target := {q(Dn(b))}, trigger := {q(Dn(t))}}})") + "\n"
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Design.lex.tex", "w").write(out)

# ---------------------------------------------------------------- Features
F = "Features"
calls = [
    ("callProject", "uorust project WORKSPACE --out DIR"),
    ("callCheck", "uorust check WORKSPACE --out DIR"),
    ("callRoutes", "uorust routes"),
    ("callProjectWorkspace", "uorust::project_workspace"),
    ("callRespond", "uorust::route::respond"),
    ("callPrismPmCheck", "prismpm --project DIR check"),
    ("callPagesRoute", "the Route operation of the Pages workbench"),
]
accepts = [
    ("acceptFixtureProjection", "UE-01: the fixture workspace projects into exactly the committed Prism project, which prismpm check accepts"),
    ("acceptByteIdentical", "UE-02: two projections of the same workspace are byte-identical"),
    ("acceptTypedRefusals", "UE-03: admission refuses ambiguous and incomplete facts as open claims"),
    ("acceptLift", "UE-04: total functions lift to LexLean definitions; the rest is typed residue"),
    ("acceptRouterConformance", "UE-05: the Rust router answers every vector of model/routes.toml"),
    ("acceptTypedErrors", "UE-06: every failed run is a typed UorustError with a negative test"),
    ("acceptGate", "UE-07: just vv checks the fixture and self projections with prismpm check"),
    ("acceptRouteTheorems", "UR-01: the route table is total and refuses ambiguity, by decide"),
]
features = [
    ("featureProjectWorkspace", "Project a Cargo workspace into a complete Prism project in one command.", ["callProject", "callProjectWorkspace"], ["acceptFixtureProjection", "acceptGate"]),
    ("featureCheckProjection", "Check that a directory holds exactly the current projection of a workspace.", ["callCheck"], ["acceptFixtureProjection", "acceptByteIdentical"]),
    ("featureRouteFacts", "Route each kind of Rust fact to the one Prism record kind it becomes, or refuse it with a reason.", ["Routes::dispatch", "callRespond", "callRoutes"], ["Routes::acceptanceVectors", "acceptRouterConformance", "acceptRouteTheorems"]),
    ("featureRefuseAmbiguity", "Refuse an ambiguous or unknown fact as an open claim instead of choosing a record by default.", ["callProjectWorkspace", "callRespond"], ["acceptTypedRefusals", "acceptRouteTheorems"]),
    ("featureLiftBehavior", "Lift total Rust functions to LexLean definitions and decide their closed assertions as theorems.", ["callProjectWorkspace"], ["acceptLift"]),
    ("featureDeterministicOutput", "Write byte-identical output on every run, so a projection can be committed and checked.", ["callProject", "callCheck"], ["acceptByteIdentical"]),
    ("featureTypedErrors", "End every failed run with a typed error that names its cause.", ["callProject", "callCheck"], ["acceptTypedErrors"]),
    ("featureCheckedOutput", "Emit a project the locked PrismPM SDK accepts.", ["callPrismPmCheck"], ["acceptFixtureProjection", "acceptGate"]),
    ("featureSelfModel", "Model UORust itself in every Prism facet and verify that model with the same SDK.", ["callPrismPmCheck"], ["acceptGate", "acceptRouteTheorems"]),
    ("featureBrowserRoutes", "Run the verified route table in the browser on the Pages site.", ["callPagesRoute", "Routes::dispatch"], ["Routes::acceptanceVectors"]),
]
def ref(x): return x if "::" in x else f"Features::{x}"
journeys = [
    ("journeyRustDeveloper", "stakeholderRustDeveloper", ["featureProjectWorkspace", "featureCheckedOutput", "featureDeterministicOutput", "featureCheckProjection"]),
    ("journeyPrismModeler", "stakeholderPrismModeler", ["featureRouteFacts", "featureRefuseAmbiguity", "featureLiftBehavior", "featureCheckedOutput"]),
    ("journeySecurityReviewer", "stakeholderSecurityReviewer", ["featureProjectWorkspace", "featureRefuseAmbiguity", "featureTypedErrors"]),
    ("journeyDevRelAuthor", "stakeholderDevRelAuthor", ["featureBrowserRoutes", "featureRouteFacts", "featureSelfModel"]),
    ("journeyMaintainer", "stakeholderMaintainer", ["featureSelfModel", "featureRouteFacts", "featureDeterministicOutput", "featureCheckProjection"]),
]
out = header(F, "UORust features and journeys", ["Architecture", "Foundation.Arch", "Routes"])
for n, v in calls + accepts:
    out += defn(n, "String", q(v)) + "\n"
for n, st, cs, acs in features:
    out += defn(n, "Foundation.Arch::Feature", f"(record Foundation.Arch::Feature {{statement := {q(st)}, calls := {strlist([ref(c) for c in cs])}, acceptance := {strlist([ref(a) for a in acs])}}})") + "\n"
for n, actor, steps in journeys:
    out += defn(n, "Foundation.Arch::Journey", f"(record Foundation.Arch::Journey {{actor := {q(C(actor))}, steps := {strlist(['Features::' + s for s in steps])}}})") + "\n"
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Features.lex.tex", "w").write(out)

# ---------------------------------------------------------------- Security
S = "Security"
assets = [
    ("assetSubjectSource", "componentSubjectWorkspace", "Subject source code", "The manifests, lock file and Rust sources uorust reads. They may be private and must not leave the machine."),
    ("assetEmittedModel", "componentEmitter", "Emitted Prism model", "The Prism project uorust writes. Reviewers trust it to state what the source does."),
    ("assetSupplyChain", "componentCratesIo", "Supply chain", "The locked crates uorust is built from, and the crates the subject workspace locks."),
    ("assetReleaseBinary", "componentCli", "Release binary", "The uorust binary developers download and run against their workspaces."),
    ("assetCi", "componentGitHubActions", "CI pipeline", "The GitHub Actions workflows that gate every push and deploy the Pages site."),
    ("assetPagesSite", "componentPagesSite", "Pages site", "The published page and the route-table guest it runs."),
]
L = {"low": "low", "medium": "medium", "high": "high"}
threats = [
    # name, asset, likelihood, impact, title, text, stride
    ("threatBuildScriptExecution", "assetSubjectSource", "medium", "critical", "Code execution from the subject", "A build script or procedural macro in the subject workspace runs on the reviewer's machine while it is modeled.", "elevationOfPrivilege"),
    ("threatSourceExfiltration", "assetSubjectSource", "low", "critical", "Source exfiltration", "uorust or one of its dependencies sends the subject's source to a third party.", "informationDisclosure"),
    ("threatCraftedSourceExhaustion", "assetEmittedModel", "medium", "minor", "Crafted source exhausting uorust", "A huge or deeply nested source file consumes memory or time until no model is written.", "denialOfService"),
    ("threatMisroutedFact", "assetEmittedModel", "medium", "major", "Misrouted fact", "A fact becomes the wrong record, or an ambiguous fact is resolved by a default, so the model misstates the source.", "tampering"),
    ("threatDisputedProjection", "assetEmittedModel", "low", "minor", "Disputed projection", "A user disputes which model uorust produced for a given commit.", "repudiation"),
    ("threatDependencyCompromise", "assetSupplyChain", "low", "critical", "Compromised dependency", "A crate uorust is built from is replaced by a malicious or vulnerable version.", "tampering"),
    ("threatUnsafeMemory", "assetReleaseBinary", "low", "major", "Memory corruption", "Unsafe code in uorust corrupts memory while parsing hostile source.", "elevationOfPrivilege"),
    ("threatPathEscape", "assetReleaseBinary", "low", "major", "Write outside the output directory", "A crafted crate name makes the emitter write outside the directory given by --out.", "tampering"),
    ("threatWorkflowTokenAbuse", "assetCi", "low", "major", "Workflow token abuse", "A third-party action or a persisted credential lets an attacker push or publish from CI.", "elevationOfPrivilege"),
    ("threatTamperedPages", "assetPagesSite", "low", "major", "Tampered Pages artifact", "The published page or guest is altered so it routes facts differently from the verified table.", "tampering"),
    ("threatPagesHostSubstitution", "assetPagesSite", "low", "major", "Compromised Pages host", "The host serves a self-consistent altered artifact whose digests match.", "spoofing"),
]
controls = [
    # name, threat(for riskIndex), title, text, evidence [(kind, ref)]
    ("controlNoBuildExecution", "threatBuildScriptExecution", "No build of the subject", "uorust reads the workspace through cargo metadata and syn only; it never compiles the subject or runs its build scripts or macros.", [("review", "crates/uorust/src/facts.rs reads sources with syn::parse_file"), ("gate", "UE-01 projects the fixture workspace without building it")]),
    ("controlOffline", "threatSourceExfiltration", "No network", "uorust links no network client, and cargo metadata runs with --offline against the subject's lock file.", [("gate", "cargo deny check over Cargo.lock"), ("review", "no std::net use in crates/uorust")]),
    ("controlBoundedIngest", "threatCraftedSourceExhaustion", "Bounded ingest", "Every read is bounded in size and every walk in depth; a bound that is hit ends the run with a typed error.", [("gate", "UE-06 negative tests for every UorustError variant")]),
    ("controlVerifiedRouteTable", "threatMisroutedFact", "Verified route table", "The route table is a LexLean function proved total and ambiguity-refusing by decide, and checked by the Lean kernel.", [("proof", "Routes::routeTableIsTotal"), ("proof", "Routes::routeTableRefusesAmbiguity"), ("gate", "prismpm --project app verify")]),
    ("controlRouterConformance", "threatMisroutedFact", "Router conformance", "The Rust router replays every vector of model/routes.toml, the same vectors the verified guest replays.", [("gate", "UE-05 crates/uorust/tests/routes.rs")]),
    ("controlTypedRefusal", "threatMisroutedFact", "Typed refusal", "Admission never defaults: an ambiguous, unknown or incomplete fact becomes an open claim with its reason.", [("gate", "UE-03 typed refusals recorded as open claims")]),
    ("controlDeterministicOutput", "threatDisputedProjection", "Deterministic output", "Output is a pure function of the workspace and its commit time, written in a fixed order, so a dispute is settled by re-running uorust.", [("gate", "UE-02 two projections are byte-identical")]),
    ("controlCargoDeny", "threatDependencyCompromise", "Dependency policy", "cargo deny refuses advisories, unknown registries, wildcard requirements and licenses outside Apache-2.0, MIT and Unicode-3.0.", [("gate", "just vv deny")]),
    ("controlLockedBuild", "threatDependencyCompromise", "Locked build", "Every build and test runs with --locked against the committed Cargo.lock.", [("gate", "just vv lint and test run cargo with --locked")]),
    ("controlForbidUnsafe", "threatUnsafeMemory", "No unsafe code", "#![forbid(unsafe_code)] in every uorust crate makes any unsafe block a compile error.", [("gate", "rustc forbid(unsafe_code) in crates/uorust/src/main.rs")]),
    ("controlConfinedOutput", "threatPathEscape", "Confined output", "The emitter writes only relative paths it generated itself, each checked to stay below the output directory.", [("review", "crates/uorust/src/lib.rs write")]),
    ("controlPinnedWorkflows", "threatWorkflowTokenAbuse", "Pinned workflows", "Workflows pin every action by commit, check out with persist-credentials false and are themselves pinned by template.lock.", [("gate", "xtask audit-bootstrap")]),
    ("controlArtifactClosure", "threatTamperedPages", "Artifact closure", "prismpm verify replays the acceptance vectors in the built guest and checks the exact file closure of the browser artifact.", [("gate", "prismpm --project app verify")]),
]
accepted = {
    "threatPagesHostSubstitution": "The page cannot defend against its own host. A user who needs more can build the route table from the tagged source with prismpm verify and compare.",
}
mapped = {}
for n, t, *_ in controls:
    mapped.setdefault(t, []).append(n)
assert set(mapped) | set(accepted) == {t[0] for t in threats}
ai = indexed([a[0] for a in assets]); ti = indexed([t[0] for t in threats])
risks = [("risk" + t[0][6:], t) for t in threats]
ri = indexed([r[0] for r in risks]); ki = indexed([c[0] for c in controls])
risk_of = {t[0]: rn for rn, t in risks}
measured = [c for c in controls if any(k == "gate" for k, _ in c[4])]
mi = indexed(["measurement" + c[0][7:] for c in measured])
acts = indexed(["activity" + c[0][7:] for c in controls])

out = header(S, "UORust threat model", ["Architecture", "Foundation.Sec", "Production.ThreatModel"], ("prism.arch", "prism.sec"))
for n, comp, *_ in assets:
    out += defn(n, "Foundation.Sec::Asset", f"(record Foundation.Sec::Asset {{index := {ai[n]}, componentIndex := {pi[comp]}}})") + "\n"
for t in threats:
    out += defn(t[0], "Foundation.Sec::Threat", f"(record Foundation.Sec::Threat {{index := {ti[t[0]]}}})") + "\n"
for rn, t in risks:
    out += defn(rn, "Foundation.Sec::Risk", f"(record Foundation.Sec::Risk {{index := {ri[rn]}, assetIndex := {ai[t[1]]}, threatIndex := {ti[t[0]]}, likelihood := (ctor Foundation.Sec::Likelihood.{t[2]}), impact := (ctor Foundation.Sec::Impact.{t[3]})}})") + "\n"
for c in controls:
    out += defn(c[0], "Foundation.Sec::ApplicationSecurityControl", f"(record Foundation.Sec::ApplicationSecurityControl {{index := {ki[c[0]]}, riskIndex := {ri[risk_of[c[1]]]}}})") + "\n"
for c in controls:
    n = "activity" + c[0][7:]
    out += defn(n, "Foundation.Sec::SecurityActivity", f"(record Foundation.Sec::SecurityActivity {{index := {acts[n]}, controlIndex := {ki[c[0]]}}})") + "\n"
for c in measured:
    n = "measurement" + c[0][7:]
    out += defn(n, "Foundation.Sec::VerificationMeasurement", f"(record Foundation.Sec::VerificationMeasurement {{index := {mi[n]}, controlIndex := {ki[c[0]]}, passed := true}})") + "\n"
out += defn("threatModelControlSet", "Production.ThreatModel::ControlSet", '(record Production.ThreatModel::ControlSet {title := "UORust security controls", text := "uorust reads a Cargo workspace and writes a Prism project. It runs locally, builds nothing it reads, opens no network connection and writes only below its output directory.", version := "0.1.0", lastModified := "2026-11-11T00:00:00Z"})') + "\n"
for n, comp, name, text in assets:
    out += defn("modeled" + n[0].upper() + n[1:], "Production.ThreatModel::ModeledAsset", f"(record Production.ThreatModel::ModeledAsset {{asset := {q('Security::' + n)}, name := {q(name)}, text := {q(text)}}})") + "\n"
for t in threats:
    out += defn("modeled" + t[0][0].upper() + t[0][1:], "Production.ThreatModel::ModeledThreat", f"(record Production.ThreatModel::ModeledThreat {{threat := {q('Security::' + t[0])}, name := {q(t[4])}, text := {q(t[5])}, category := (ctor Production.ThreatModel::Stride.{t[6]})}})") + "\n"
for c in controls:
    ev = "(nil Production.ThreatModel::Evidence)"
    for kind, r in reversed(c[4]):
        ev = f"(cons (record Production.ThreatModel::Evidence {{kind := (ctor Production.ThreatModel::EvidenceKind.{kind}), reference := {q(r)}}}) {ev})"
    out += defn("modeled" + c[0][0].upper() + c[0][1:], "Production.ThreatModel::ModeledControl", f"(record Production.ThreatModel::ModeledControl {{control := {q('Security::' + c[0])}, name := {q(c[2])}, text := {q(c[3])}, evidence := {ev}}})") + "\n"
for t, cs in mapped.items():
    out += defn("mapping" + t[6:], "Production.ThreatModel::ThreatMapping", f"(record Production.ThreatModel::ThreatMapping {{threat := {q('Security::' + t)}, controls := {strlist(['Security::' + c for c in cs])}}})") + "\n"
for t, why in accepted.items():
    out += defn("accepted" + t[6:], "Production.ThreatModel::AcceptedRisk", f"(record Production.ThreatModel::AcceptedRisk {{threat := {q('Security::' + t)}, rationale := {q(why)}}})") + "\n"
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Security.lex.tex", "w").write(out)

# ---------------------------------------------------------------- Quality
Qm = "Quality"
chars = ["characteristicFunctionalSuitability", "characteristicPerformanceEfficiency", "characteristicReliability",
         "characteristicMaintainability", "characteristicFlexibility", "characteristicSecurity"]
subs = [("subcharacteristicFunctionalCompleteness", "characteristicFunctionalSuitability"),
        ("subcharacteristicFunctionalCorrectness", "characteristicFunctionalSuitability"),
        ("subcharacteristicTimeBehaviour", "characteristicPerformanceEfficiency"),
        ("subcharacteristicCapacity", "characteristicPerformanceEfficiency"),
        ("subcharacteristicFaultlessness", "characteristicReliability"),
        ("subcharacteristicTestability", "characteristicMaintainability"),
        ("subcharacteristicModifiability", "characteristicMaintainability"),
        ("subcharacteristicAdaptability", "characteristicFlexibility"),
        ("subcharacteristicInstallability", "characteristicFlexibility"),
        ("subcharacteristicIntegrity", "characteristicSecurity")]
reqs = [  # requirement, sub, measure, threshold
    ("requirementRouteCoverage", "subcharacteristicFunctionalCompleteness", "measureRoutedFactKinds", 28),
    ("requirementVectorCoverage", "subcharacteristicFunctionalCorrectness", "measureRouteVectors", 39),
    ("requirementSelfFixpoint", "subcharacteristicFunctionalCorrectness", "measureSelfModelDrift", 0),
    ("requirementDeterminism", "subcharacteristicFaultlessness", "measureRerunDifferingBytes", 0),
    ("requirementSelfProjectionTime", "subcharacteristicTimeBehaviour", "measureSelfProjectionSeconds", 10),
    ("requirementRequestBound", "subcharacteristicCapacity", "measureRouteRequestBytes", 512),
    ("requirementRegisteredTests", "subcharacteristicTestability", "measureUnregisteredTests", 0),
    ("requirementLintClean", "subcharacteristicModifiability", "measureClippyWarnings", 0),
    ("requirementMsrv", "subcharacteristicAdaptability", "measureMinimumRustMinor", 89),
    ("requirementStaticRouteCli", "subcharacteristicInstallability", "measureRouteCliNeededLibraries", 0),
    ("requirementNoUnsafe", "subcharacteristicIntegrity", "measureUnsafeBlocks", 0),
]
chi = indexed(chars); subi = indexed([s for s, _ in subs]); rqi = indexed([r[0] for r in reqs]); msi = indexed([r[2] for r in reqs])
out = header(Qm, "UORust quality model", ["Foundation.Qual"], ("prism.arch", "prism.qual"))
for c in chars:
    out += defn(c, "Foundation.Qual::ProductQualityCharacteristic", f"(record Foundation.Qual::ProductQualityCharacteristic {{index := {chi[c]}}})") + "\n"
for s_, c in subs:
    out += defn(s_, "Foundation.Qual::QualitySubcharacteristic", f"(record Foundation.Qual::QualitySubcharacteristic {{index := {subi[s_]}, characteristicIndex := {chi[c]}}})") + "\n"
for r, s_, m, t in reqs:
    out += defn(r, "Foundation.Qual::QualityRequirement", f"(record Foundation.Qual::QualityRequirement {{index := {rqi[r]}, subcharacteristicIndex := {subi[s_]}}})") + "\n"
for r, s_, m, t in reqs:
    out += defn(m, "Foundation.Qual::QualityMeasure", f"(record Foundation.Qual::QualityMeasure {{index := {msi[m]}, requirementIndex := {rqi[r]}, threshold := {t}}})") + "\n"
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Quality.lex.tex", "w").write(out)

counts = dict(stakeholders=len(stakeholders), concerns=len(concerns), components=len(components), edges=len(edges),
              viewpoints=len(viewpoints), views=len(views), processes=len(processes), states=len(states), designViews=len(dviews),
              opmThings=len(opm_things), opmLinks=len(links), umlMessages=len(msgs), umlTransitions=len(trans),
              features=len(features), journeys=len(journeys), assets=len(assets), threats=len(threats), controls=len(controls),
              accepted=len(accepted), characteristics=len(chars), subcharacteristics=len(subs), requirements=len(reqs), measures=len(reqs))
print(counts)
