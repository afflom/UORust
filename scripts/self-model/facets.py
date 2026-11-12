"""Emit UORust's facet modules (Architecture, Design, Features, Security, Quality)
with every index equal to the record's position in identifier order.

What the sources state is read from them, not restated: the text of the
components and features UORust's code documents, the register rows that are
the features' acceptance and the controls' gate evidence, the source lines a
review names, and every quality threshold. Each such record is listed with the
span it was read from and that span's κ (Architecture::Source), so a change of
the source is a change of the model that `just self-model-check` reports.

    python3 -I facets.py OUT_ROOT [SOURCE_ROOT]
"""
import hashlib, re, sys, tomllib

root = sys.argv[1]
SRC = sys.argv[2] if len(sys.argv) > 2 else root
OUT = f"{root}/app/src"

# ---------------------------------------------------------------- Sources
SPAN_DOMAIN = "uorust/span/1"
SOURCES = {}  # module -> [(record, path, first, last, kappa)]

def lines_of(path):
    return open(f"{SRC}/{path}", encoding="utf-8").read().split("\n")

def kappa(path, first, last):
    """κ of a span: SHA-256 of the domain, the path and the span's lines."""
    text = "\n".join(lines_of(path)[first - 1:last])
    return hashlib.sha256(f"{SPAN_DOMAIN}\0{path}\0{text}".encode()).hexdigest()

def cite(module, record, path, first, last):
    SOURCES.setdefault(module, []).append((record, path, first, last, kappa(path, first, last)))

def plain(text):
    """Rust doc Markdown to plain text: links and code spans keep their words."""
    text = re.sub(r"\[`?([^`\]]+)`?\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[`([^`\]]+)`\]", r"\1", text)
    return text.replace("`", "")

def doc_sentence(path, first, marker):
    """The first sentence of the doc comment that starts at line `first`: its
    text and the last line it spans."""
    lines = lines_of(path)
    words, at = [], first - 1
    while at < len(lines) and lines[at].strip().startswith(marker):
        body = lines[at].strip()[len(marker):].strip()
        assert body, f"{path}:{at + 1}: a doc sentence ends before a blank doc line"
        words.append(body)
        if re.search(r"\.(\s|$)", body):
            break
        at += 1
    text = " ".join(words)
    end = re.search(r"\.(\s|$)", text)
    assert end, f"{path}:{first}: no sentence"
    return plain(text[: end.start() + 1]), at + 1

def module_doc(module, record, path):
    text, last = doc_sentence(path, 1, "//!")
    cite(module, record, path, 1, last)
    return text

def item_doc(module, record, path, item):
    """The first sentence of the `///` doc of the item whose line starts with `item`."""
    lines = lines_of(path)
    at = next(i for i, l in enumerate(lines) if l.strip().startswith(item))
    while lines[at - 1].strip().startswith("#["):
        at -= 1
    top = at
    while lines[top - 1].strip().startswith("///"):
        top -= 1
    assert top < at, f"{path}: {item} has no doc"
    text, last = doc_sentence(path, top + 1, "///")
    cite(module, record, path, top + 1, last)
    return text

def line_of(module, record, path, needle):
    """The one line of `path` holding `needle`, cited; its number."""
    hits = [i + 1 for i, l in enumerate(lines_of(path)) if needle in l]
    assert len(hits) == 1, f"{path}: {needle!r} on lines {hits}"
    cite(module, record, path, hits[0], hits[0])
    return hits[0]

# The release the control set describes: the workspace version.
WORKSPACE_VERSION = tomllib.loads(open(f"{SRC}/Cargo.toml", encoding="utf-8").read())["workspace"]["package"]["version"]

IDS = {r["id"]: r for r in tomllib.loads(open(f"{SRC}/model/ids.toml", encoding="utf-8").read())["id"]}

def register_row(module, record, rid):
    """A register row's statement, cited by its statement line."""
    lines = lines_of("model/ids.toml")
    at = lines.index(f'id = "{rid}"')
    stmt = next(i for i in range(at, len(lines)) if lines[i].startswith("statement = "))
    cite(module, record, "model/ids.toml", stmt + 1, stmt + 1)
    return IDS[rid]["statement"]

def clap_call(module, record, sub):
    """The command line of a `uorust` subcommand, read from its clap declaration:
    the variant's argument struct gives the positional and required options."""
    path = "crates/uorust/src/main.rs"
    lines = lines_of(path)
    variant = sub[0].upper() + sub[1:]
    at = next(i for i, l in enumerate(lines) if re.match(rf"\s+{variant}(\(|,)", l))
    cite(module, record, path, at + 1, at + 1)
    words = ["uorust", sub]
    m = re.match(rf"\s+{variant}\((\w+)\),", lines[at])
    if m:
        start = next(i for i, l in enumerate(lines) if l.startswith(f"struct {m.group(1)} "))
        end = next(i for i in range(start, len(lines)) if lines[i] == "}")
        cite(module, record + "Arguments", path, start + 1, end + 1)
        long = False
        for l in lines[start:end]:
            l = l.strip()
            if l.startswith("#[arg(long"):
                long = True
                continue
            f = re.match(r"(\w+): (\w+)", l)
            if f:
                optional = f.group(2) == "Option"
                if not optional:
                    words.append(f"--{f.group(1).replace('_', '-')} {f.group(1).upper()}" if long else f.group(1).upper())
                long = False
    return " ".join(words)

def source_list(module):
    o = "(nil Architecture::Source)"
    for rec, path, first, last, k in reversed(SOURCES.get(module, [])):
        o = (f"(cons (record Architecture::Source {{record := {q(rec)}, path := {q(path)}, firstLine := {first}, "
             f"lastLine := {last}, kappa := {q(k)}}}) {o})")
    return o

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
    ("componentCli", "releaseBinary", "uorust CLI", item_doc(A, "Architecture::componentCli", "crates/uorust/src/main.rs", "struct Cli ")),
    ("componentIngest", "service", "Ingest", module_doc(A, "Architecture::componentIngest", "crates/uorust/src/ingest.rs")),
    ("componentRouter", "service", "Router", module_doc(A, "Architecture::componentRouter", "crates/uorust/src/route.rs")),
    ("componentAdmission", "service", "Admission", module_doc(A, "Architecture::componentAdmission", "crates/uorust/src/admit.rs")),
    ("componentEmitter", "service", "Emitter", module_doc(A, "Architecture::componentEmitter", "crates/uorust/src/project.rs")),
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
out += """structure{
  name: Source,
  type_parameters: [],
  parameters: [],
  fields: [{name: record, type: String}, {name: path, type: String}, {name: firstLine, type: Nat}, {name: lastLine, type: Nat}, {name: kappa, type: String}]
}

"""
out += defn("sourcesOfComponents", "(List Source)", source_list(A).replace("Architecture::Source", "Source")) + "\n"
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
FS = "Features::"
calls = [
    ("callProject", clap_call(F, FS + "callProject", "project")),
    ("callCheck", clap_call(F, FS + "callCheck", "check")),
    ("callRoutes", clap_call(F, FS + "callRoutes", "routes")),
    ("callProjectWorkspace", "uorust::project_workspace"),
    ("callRespond", "uorust::route::respond"),
    ("callPrismPmCheck", "prismpm --project DIR check"),
    ("callPagesRoute", "the Route operation of the Pages workbench"),
]
# The library calls are the public functions they name.
line_of(F, FS + "callProjectWorkspace", "crates/uorust/src/lib.rs", "pub fn project_workspace(")
line_of(F, FS + "callRespond", "crates/uorust/src/route.rs", "pub fn respond(")
# Each acceptance is a register row, stated as the register states it.
accept_rows = [
    ("acceptFixtureProjection", "UE-01"), ("acceptByteIdentical", "UE-02"), ("acceptTypedRefusals", "UE-03"),
    ("acceptLift", "UE-04"), ("acceptRouterConformance", "UE-05"), ("acceptTypedErrors", "UE-06"),
    ("acceptGate", "UE-07"), ("acceptRouteTheorems", "SM-02"),
]
accepts = [(n, f"{rid}: {register_row(F, FS + n, rid)}") for n, rid in accept_rows]
# A feature's statement is what the code or the register says it does.
features = [
    ("featureProjectWorkspace", item_doc(F, FS + "featureProjectWorkspace", "crates/uorust/src/main.rs", "Project(Args),"), ["callProject", "callProjectWorkspace"], ["acceptFixtureProjection", "acceptGate"]),
    ("featureCheckProjection", item_doc(F, FS + "featureCheckProjection", "crates/uorust/src/main.rs", "Check(Args),"), ["callCheck"], ["acceptFixtureProjection", "acceptByteIdentical"]),
    ("featureRouteFacts", module_doc(F, FS + "featureRouteFacts", "crates/uorust/src/route.rs"), ["Routes::dispatch", "callRespond", "callRoutes"], ["Routes::acceptanceVectors", "acceptRouterConformance", "acceptRouteTheorems"]),
    ("featureRefuseAmbiguity", module_doc(F, FS + "featureRefuseAmbiguity", "crates/uorust/src/admit.rs"), ["callProjectWorkspace", "callRespond"], ["acceptTypedRefusals", "acceptRouteTheorems"]),
    ("featureLiftBehavior", module_doc(F, FS + "featureLiftBehavior", "crates/uorust/src/lift.rs"), ["callProjectWorkspace"], ["acceptLift"]),
    ("featureDeterministicOutput", register_row(F, FS + "featureDeterministicOutput", "UE-02"), ["callProject", "callCheck"], ["acceptByteIdentical"]),
    ("featureTypedErrors", register_row(F, FS + "featureTypedErrors", "UE-06"), ["callProject", "callCheck"], ["acceptTypedErrors"]),
    ("featureCheckedOutput", register_row(F, FS + "featureCheckedOutput", "UE-07"), ["callPrismPmCheck"], ["acceptFixtureProjection", "acceptGate"]),
    ("featureSelfModel", register_row(F, FS + "featureSelfModel", "SM-03"), ["callPrismPmCheck"], ["acceptGate", "acceptRouteTheorems"]),
    ("featureBrowserRoutes", register_row(F, FS + "featureBrowserRoutes", "SM-01"), ["callPagesRoute", "Routes::dispatch"], ["Routes::acceptanceVectors"]),
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
out += defn("sourcesOfFeatures", "(List Architecture::Source)", source_list(F)) + "\n"
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Features.lex.tex", "w").write(out)

# ---------------------------------------------------------------- Security
S = "Security"
SS = "Security::modeled"
def gate(control, rid):
    """Gate evidence that is a register row: the row's ID and statement, cited."""
    return ("gate", f"{rid}: {register_row(S, SS + control[0].upper() + control[1:], rid)}")
def review(control, path, needle, kind="review"):
    """Evidence that is one source line: its path, number and text, cited."""
    at = line_of(S, SS + control[0].upper() + control[1:], path, needle)
    return (kind, f"{path}:{at} {lines_of(path)[at - 1].strip()}")
def absent(control, needle):
    """Review evidence that no line of uorust's code (doc comments aside) names `needle`."""
    import os
    base = "crates/uorust/src"
    for d, _, fs in sorted(os.walk(f"{SRC}/{base}")):
        for f in sorted(fs):
            rel = os.path.relpath(f"{d}/{f}", SRC)
            for i, l in enumerate(lines_of(rel)):
                assert needle not in l or l.strip().startswith("//"), f"{rel}:{i + 1} names {needle}"
    return ("review", f"no line of {base} names {needle} outside a doc comment (checked by scripts/self-model/facets.py)")
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
    ("controlNoBuildExecution", "threatBuildScriptExecution", "No build of the subject but its MIR dump", "uorust project, check and prove read the workspace through cargo metadata and syn only and never compile the subject; uorust mir alone compiles it with cargo, running its build scripts and macros as any build of it does, to dump the MIR the lift translates, and it is run only on a workspace the reviewer would build.", [review("controlNoBuildExecution", "crates/uorust/src/ingest.rs", "syn::parse_file(&text)"), review("controlNoBuildExecution", "crates/uorust/src/mir/mod.rs", '.env("RUSTC_BOOTSTRAP", "1")'), gate("controlNoBuildExecution", "UE-01")]),
    ("controlOffline", "threatSourceExfiltration", "No network", "uorust links no network client, and every cargo command it runs (cargo metadata, and the MIR dump's cargo clean and cargo rustc) runs with --offline against the subject's lock file.", [("gate", "cargo deny check over Cargo.lock"), absent("controlOffline", "std::net")]),
    ("controlBoundedIngest", "threatCraftedSourceExhaustion", "Bounded ingest", "Every read is bounded in size and every walk in depth; a bound that is hit ends the run with a typed error.", [gate("controlBoundedIngest", "UE-06")]),
    ("controlVerifiedRouteTable", "threatMisroutedFact", "Verified route table", "The route table is a LexLean function proved total and ambiguity-refusing by decide, and checked by the Lean kernel.", [("proof", "Routes::routeTableIsTotal"), ("proof", "Routes::routeTableRefusesAmbiguity"), ("gate", "prismpm --project app verify")]),
    ("controlRouterConformance", "threatMisroutedFact", "Router conformance", "The Rust router replays every vector of model/routes.toml, the same vectors the verified guest replays.", [gate("controlRouterConformance", "UE-05")]),
    ("controlTypedRefusal", "threatMisroutedFact", "Typed refusal", "Admission never defaults: an ambiguous, unknown or incomplete fact becomes an open claim with its reason.", [gate("controlTypedRefusal", "UE-03")]),
    ("controlDeterministicOutput", "threatDisputedProjection", "Deterministic output", "Output is a pure function of the workspace and its commit time, written in a fixed order, so a dispute is settled by re-running uorust.", [gate("controlDeterministicOutput", "UE-02")]),
    ("controlCargoDeny", "threatDependencyCompromise", "Dependency policy", "cargo deny refuses advisories, unknown registries, wildcard requirements and licenses outside Apache-2.0, MIT and Unicode-3.0.", [("gate", "just vv deny")]),
    ("controlLockedBuild", "threatDependencyCompromise", "Locked build", "Every build and test runs with --locked against the committed Cargo.lock.", [("gate", "just vv lint and test run cargo with --locked")]),
    ("controlForbidUnsafe", "threatUnsafeMemory", "No unsafe code", "#![forbid(unsafe_code)] in every uorust crate makes any unsafe block a compile error.", [review("controlForbidUnsafe", "crates/uorust/src/main.rs", "#![forbid(unsafe_code)]", "gate"), review("controlForbidUnsafe", "crates/uorust/src/lib.rs", "#![forbid(unsafe_code)]", "gate")]),
    ("controlConfinedOutput", "threatPathEscape", "Confined output", "The emitter writes only relative paths it generated itself, each checked to stay below the output directory.", [review("controlConfinedOutput", "crates/uorust/src/lib.rs", "pub fn write(out: &Path")]),
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
out += defn("threatModelControlSet", "Production.ThreatModel::ControlSet", '(record Production.ThreatModel::ControlSet {title := "UORust security controls", text := "uorust reads a Cargo workspace and writes a Prism project. It runs locally, builds nothing it reads, opens no network connection and writes only below its output directory.", version := ' + q(WORKSPACE_VERSION) + ', lastModified := "2026-11-11T00:00:00Z"})') + "\n"
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
out += defn("sourcesOfControls", "(List Architecture::Source)", source_list(S)) + "\n"
for t, why in accepted.items():
    out += defn("accepted" + t[6:], "Production.ThreatModel::AcceptedRisk", f"(record Production.ThreatModel::AcceptedRisk {{threat := {q('Security::' + t)}, rationale := {q(why)}}})") + "\n"
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Security.lex.tex", "w").write(out)

# ---------------------------------------------------------------- Quality
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
# Every threshold is derived, never chosen here:
# - counted from the route table (routes, router vectors) or read from the
#   manifest (the MSRV), with the span it was read from;
# - the model's own bound (the request limit, Routes::requestLimit);
# - the measurement of a comparator pinned in model/comparators.toml;
# - or 0, for a measure that counts defects (drift, differing bytes,
#   unregistered tests, warnings, needed libraries, unsafe blocks, unlifted
#   functions): the requirement is that there are none, and the excess over 0
#   is the distance to it.
Qm = "Quality"
QS = "Quality::"
ROUTES_TOML = "model/routes.toml"
routes_lines = lines_of(ROUTES_TOML)
route_rows = [i + 1 for i, l in enumerate(routes_lines) if l == "[[route]]"]
vector_rows = [i + 1 for i, l in enumerate(routes_lines) if l == "[[vector]]"]
cite(Qm, QS + "measureRoutedFactKinds", ROUTES_TOML, route_rows[0], vector_rows[0] - 1)
cite(Qm, QS + "measureRouteVectors", ROUTES_TOML, vector_rows[0], vector_rows[-1] + 3)
msrv_line = line_of(Qm, QS + "measureMinimumRustMinor", "Cargo.toml", "rust-version = ")
msrv_minor = int(re.search(r'"1\.(\d+)', lines_of("Cargo.toml")[msrv_line - 1]).group(1))
routes_lex = open(f"{root}/app/src/Routes.lex.tex", encoding="utf-8").read()
request_limit = int(re.search(r"name: requestLimit,\s*parameters: \[\],\s*result: Nat,\s*body: (\d+)", routes_lex).group(1))
COMPARATORS = "model/comparators.toml"
comparators = tomllib.loads(open(f"{SRC}/{COMPARATORS}", encoding="utf-8").read())
def pinned(kind, measure):
    """The one row of `kind` for `measure` in model/comparators.toml, cited."""
    rows = [r for r in comparators[kind] if r["measure"] == measure]
    assert len(rows) == 1, f"{COMPARATORS}: {len(rows)} {kind} rows for {measure}"
    lines = lines_of(COMPARATORS)
    heads = [i + 1 for i, l in enumerate(lines) if l == f"[[{kind}]]"]
    at = [h for h in heads if f'measure = "{measure}"' in lines[h]][0]
    end = next((i for i in range(at, len(lines)) if lines[i] == ""), len(lines))
    cite(Qm, QS + measure + kind.capitalize(), COMPARATORS, at, end)
    return rows[0]["milliseconds"]
projection_threshold = pinned("comparator", "measureSelfProjectionMilliseconds")
projection_observed = pinned("observation", "measureSelfProjectionMilliseconds")
reqs = [  # requirement, sub, measure, threshold
    ("requirementRouteCoverage", "subcharacteristicFunctionalCompleteness", "measureRoutedFactKinds", len(route_rows)),
    ("requirementVectorCoverage", "subcharacteristicFunctionalCorrectness", "measureRouteVectors", len(vector_rows)),
    ("requirementSelfFixpoint", "subcharacteristicFunctionalCorrectness", "measureSelfModelDrift", 0),
    ("requirementDeterminism", "subcharacteristicFaultlessness", "measureRerunDifferingBytes", 0),
    ("requirementSelfProjectionTime", "subcharacteristicTimeBehaviour", "measureSelfProjectionMilliseconds", projection_threshold),
    ("requirementRequestBound", "subcharacteristicCapacity", "measureRouteRequestBytes", request_limit),
    ("requirementRegisteredTests", "subcharacteristicTestability", "measureUnregisteredTests", 0),
    ("requirementLintClean", "subcharacteristicModifiability", "measureClippyWarnings", 0),
    ("requirementMsrv", "subcharacteristicAdaptability", "measureMinimumRustMinor", msrv_minor),
    ("requirementStaticRouteCli", "subcharacteristicInstallability", "measureRouteCliNeededLibraries", 0),
    ("requirementNoUnsafe", "subcharacteristicIntegrity", "measureUnsafeBlocks", 0),
    # Every function of product code lifts: the measure is the number of rows
    # of the lift register (frontier.toml) that are not lifted. Its excess over
    # 0 is the distance to the requirement, an open claim; the gate (UE-16) is
    # that no lifted κ turns unlifted, never a count.
    ("requirementEveryFunctionLifted", "subcharacteristicFunctionalCompleteness", "measureUnliftedFunctions", 0),
]
chi = indexed(chars); subi = indexed([s for s, _ in subs]); rqi = indexed([r[0] for r in reqs]); msi = indexed([r[2] for r in reqs])
out = header(Qm, "UORust quality model", ["Architecture", "Foundation.Qual", "Routes"], ("prism.arch", "prism.qual"))
for c in chars:
    out += defn(c, "Foundation.Qual::ProductQualityCharacteristic", f"(record Foundation.Qual::ProductQualityCharacteristic {{index := {chi[c]}}})") + "\n"
for s_, c in subs:
    out += defn(s_, "Foundation.Qual::QualitySubcharacteristic", f"(record Foundation.Qual::QualitySubcharacteristic {{index := {subi[s_]}, characteristicIndex := {chi[c]}}})") + "\n"
for r, s_, m, t in reqs:
    out += defn(r, "Foundation.Qual::QualityRequirement", f"(record Foundation.Qual::QualityRequirement {{index := {rqi[r]}, subcharacteristicIndex := {subi[s_]}}})") + "\n"
for r, s_, m, t in reqs:
    out += defn(m, "Foundation.Qual::QualityMeasure", f"(record Foundation.Qual::QualityMeasure {{index := {msi[m]}, requirementIndex := {rqi[r]}, threshold := {t}}})") + "\n"
# The distance from an observation to a measure's threshold, and the facts the
# ratchet stands on: no excess is exactly meeting the threshold, and the excess
# never rises while the observation does not.
out += """definition{
  name: excessOver,
  parameters: [threshold : Nat, observed : Nat],
  result: Nat,
  body: (prim subtract observed threshold : Nat)
}
"""
for name, params, statement in [
    ("noExcessIsMet", "threshold : Nat, observed : Nat", "(((excessOver threshold observed) = 0) -> (observed <= threshold))"),
    ("metHasNoExcess", "threshold : Nat, observed : Nat", "((observed <= threshold) -> ((excessOver threshold observed) = 0))"),
    ("excessFallsWithTheObservation", "threshold : Nat, before : Nat, after : Nat", "((after <= before) -> ((excessOver threshold after) <= (excessOver threshold before)))"),
    ("excessRisesOnlyWithTheObservation", "threshold : Nat, before : Nat, after : Nat", "(((excessOver threshold before) < (excessOver threshold after)) -> (before < after))"),
]:
    out += f"""
theorem{{
  name: {name},
  parameters: [{params}],
  statement: {statement},
  proof: linear_arithmetic{{definitions: [excessOver]}},
  axioms: [Quot.sound, propext]
}}
"""
# The thresholds that are bounds of the model are the model's: the request
# measure's threshold is the route table's request limit.
out += f"""
theorem{{
  name: requestThresholdIsTheTableLimit,
  parameters: [],
  statement: ((project (measureRouteRequestBytes) threshold) = (Routes::requestLimit)),
  proof: decide{{}}
}}
"""
# The pinned observation of a comparator-pinned measure, and its excess over
# the threshold: none.
out += f"""
definition{{
  name: selfProjectionObservedMilliseconds,
  parameters: [],
  result: Nat,
  body: {projection_observed}
}}

theorem{{
  name: selfProjectionHasNoExcess,
  parameters: [],
  statement: ((excessOver (project (measureSelfProjectionMilliseconds) threshold) (selfProjectionObservedMilliseconds)) = 0),
  proof: decide{{}}
}}
"""
out += "\n" + defn("sourcesOfMeasures", "(List Architecture::Source)", source_list(Qm))
out = out.rstrip("\n") + "\n" + FOOT
open(f"{OUT}/Quality.lex.tex", "w").write(out)

counts = dict(stakeholders=len(stakeholders), concerns=len(concerns), components=len(components), edges=len(edges),
              viewpoints=len(viewpoints), views=len(views), processes=len(processes), states=len(states), designViews=len(dviews),
              opmThings=len(opm_things), opmLinks=len(links), umlMessages=len(msgs), umlTransitions=len(trans),
              features=len(features), journeys=len(journeys), assets=len(assets), threats=len(threats), controls=len(controls),
              accepted=len(accepted), characteristics=len(chars), subcharacteristics=len(subs), requirements=len(reqs), measures=len(reqs))
print(counts)
