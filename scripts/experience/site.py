"""Write the product home of a Prism model, in the uor.foundation brand.

The page is the model read aloud: who the product is for and what each of
them gets done (Users), what to run (the features' command-line calls), what
it is made of (Architecture), how it is kept safe (Security) and measured
(Quality), and the brand it carries (Brand). Every line is read from app/src,
which the Lean kernel checks, and from brand/, which scripts/experience
generates; nothing on the page is written by hand. The page wears the
Prism-DevRel docs stylesheet, the uor.foundation look every Prism model's docs
share, over the tokens in brand/foundation.css.

    python3 -I site.py ROOT SITE

writes SITE/docs/index.html, SITE/docs/foundation.css, and puts a "Docs" link
checks the app page SITE/index.html links to it.
"""
import html, json, pathlib, re, shutil, sys, tomllib

ROOT = pathlib.Path(sys.argv[1])
SITE = pathlib.Path(sys.argv[2])
CONFIG = tomllib.loads((ROOT / "model/experience.toml").read_text())
SRC = ROOT / "app/src"
APP = json.loads((ROOT / "site/app.json").read_text())
LINKS = CONFIG["links"]


def esc(s):
    return html.escape(s, quote=True)


def module(name):
    return (SRC / f"{name}.lex.tex").read_text()


def records(text, kind):
    return [(m.group(1), m.group(3)) for m in re.finditer(
        r"definition\{\n  name: (\w+),\n  parameters: \[\],\n  result: ([\w.:()\s]+?),\n  body: (.*?)\n\}", text, re.S)
        if m.group(2) == kind]


def body_of(text, name):
    m = re.search(rf"definition\{{\n  name: {name},\n  parameters: \[\],\n  result: [^\n]+,\n  body: (.*?)\n\}}", text, re.S)
    if not m:
        m = re.search(rf'definition\{{name: {name}, parameters: \[\], result: String, body: (".*?")\}}', text)
    return m.group(1) if m else None


def fields(body, name):
    return [re.sub(r"\\(.)", r"\1", v) for v in re.findall(rf'\b{name} := "((?:[^"\\]|\\.)*)"', body)]


def nats(body, name):
    return [int(v) for v in re.findall(rf"\b{name} := \(?(\d+)", body)]


def words(identifier, prefix):
    """featureProjectWorkspace -> Project workspace."""
    rest = identifier[len(prefix):] if identifier.startswith(prefix) else identifier
    text = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", " ", rest).lower()
    return text[:1].upper() + text[1:]


def string(text, name):
    """The value of a String definition, or None."""
    b = body_of(text, name)
    if b is None or not b.startswith('"'):
        return None
    return json.loads(b)


arch, features_m, security, quality = module("Architecture"), module("Features"), module("Security"), module("Quality")
brand_m, users_m = module("Brand"), module("Users")
stakeholders = {n: (fields(b, "name")[0], fields(b, "text")[0]) for n, b in records(arch, "Foundation.Arch::Stakeholder")}
components = [(fields(b, "name")[0], re.search(r"ComponentKind\.(\w+)", b).group(1), fields(b, "text")[0])
              for _, b in records(arch, "Foundation.Arch::Component")]
features = {n: b for n, b in records(features_m, "Foundation.Arch::Feature")}

# The commands a user types: every String call of a feature that the product's
# own command line answers.
command = CONFIG["command"]
commands = []
for name, b in features.items():
    calls = re.search(r"calls := (\(cons .*?\(nil String\)\)+|\(nil String\))", b).group(1)
    for call in re.findall(r'"([^"]+)"', calls):
        mod, _, ident = call.partition("::")
        value = string(module(mod), ident) if mod and (SRC / f"{mod}.lex.tex").exists() else None
        if value and value.split()[0] == command and value not in [c for c, _ in commands]:
            commands.append((value, fields(b, "statement")[0]))

groups_b, goals_b, tasks_b = body_of(users_m, "groups"), body_of(users_m, "goals"), body_of(users_m, "tasks")
group_refs = re.findall(r"stakeholder := \(Architecture::(\w+)\)", groups_b)
goal_rows = [(u, j, o) for u, j, o in zip(nats(goals_b, "user"), re.findall(r"journey := \(Features::(\w+)\)", goals_b), fields(goals_b, "outcome"))]
task_rows = list(zip(nats(tasks_b, "goal"), re.findall(r"feature := \(Features::(\w+)\)", tasks_b)))
measures_b = body_of(users_m, "measures")

threats = [n for n, _ in records(security, "Foundation.Sec::Threat")]
controls = [n for n, _ in records(security, "Foundation.Sec::ApplicationSecurityControl")]
measures = [(n, nats(b, "threshold")[0]) for n, b in records(quality, "Foundation.Qual::QualityMeasure")]

elements = list(zip(re.findall(r"BrandElementKind\.(\w+)\)", body_of(brand_m, "elements")), fields(body_of(brand_m, "elements"), "text")))
tokens_b = body_of(brand_m, "tokens")
tokens = list(zip(fields(tokens_b, "name"), re.findall(r"TokenType\.(\w+)\)", tokens_b), fields(tokens_b, "value"), fields(tokens_b, "hex")))
contrasts_b = body_of(brand_m, "contrasts")
contrasts = list(zip(nats(contrasts_b, "foreground"), nats(contrasts_b, "background"), nats(contrasts_b, "observedMilli"), nats(contrasts_b, "requiredMilli")))
element = {k: t for k, t in reversed(elements)}

# ------------------------------------------------------------------ page
name = APP["name"]
lead = APP["purpose"].split(". ")[0].rstrip(".") + "."
icon_git = '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5a12 12 0 0 0-3.8 23.4c.6.1.8-.3.8-.6v-2c-3.3.7-4-1.6-4-1.6-.6-1.4-1.4-1.8-1.4-1.8-1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0C17.3 4.6 18.3 5 18.3 5c.7 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6A12 12 0 0 0 12 .5z"/></svg>'


def section(sid, eyebrow, title, intro, inner):
    return (f'<section id="{sid}" aria-labelledby="{sid}-h"><p class="eyebrow fd-mono">// {esc(eyebrow)}</p>'
            f'<h2 id="{sid}-h">{esc(title)}</h2><p class="dc-summary">{esc(intro)}</p>{inner}</section>')


def figure(spec):
    """A figure as the declarative spec scripts/content/enrich.mjs draws, with its caption and data."""
    return ('<figure class="cs-figure"><script type="application/vnd.prism.figure+json">'
            + json.dumps(spec, ensure_ascii=False).replace("<", "\\u003c") + "</script></figure>")


def table(head, rows):
    th = "".join(f"<th scope=\"col\">{esc(h)}</th>" for h in head)
    tr = "".join("<tr>" + "".join(f"<td>{c}</td>" for c in r) + "</tr>" for r in rows)
    return f'<div class="table"><table><thead><tr>{th}</tr></thead><tbody>{tr}</tbody></table></div>'


who = []
for u, journey, outcome in goal_rows:
    sname, stext = stakeholders[group_refs[u]]
    steps = [words(f, "feature") for g, f in task_rows if goal_rows[g][1] == journey]
    who.append(f'<li><div class="dc-home-card"><div class="dc-card-body"><em class="fd-mono">{esc(sname)}</em>'
               f'<strong>{esc(outcome)}</strong><span>{esc(stext)}</span><ul>'
               + "".join(f'<li>{i + 1}. {esc(s)}</li>' for i, s in enumerate(steps)) + "</ul></div></div></li>")

# The first user group's tasks, in order: what the product is for, at a glance.
u0, journey0, outcome0 = goal_rows[0]
hero = figure({
    "kind": "flow", "title": f"What a {stakeholders[group_refs[u0]][0].lower()} does with {name}",
    "caption": f"The tasks of the first user group's goal, in order. Each one is a feature of {name} with acceptance tests.",
    "steps": [{"label": words(f, "feature"), "sub": fields(features[f], "statement")[0]} for g, f in task_rows if goal_rows[g][1] == journey0],
})

# The components and the edges between them, as the architecture declares them.
comp_name = {nats(b, "index")[0]: fields(b, "name")[0] for _, b in records(arch, "Foundation.Arch::Component")}
comp_kind = {nats(b, "index")[0]: re.search(r"ComponentKind\.(\w+)", b).group(1) for _, b in records(arch, "Foundation.Arch::Component")}
edges = [(nats(b, "fromIndex")[0], nats(b, "toIndex")[0], re.search(r"EdgeKind\.(\w+)", b).group(1)) for _, b in records(arch, "Foundation.Arch::Edge")]
linked = sorted({i for f, t, _ in edges for i in (f, t)})
wiring = figure({
    "kind": "graph", "title": f"How the parts of {name} connect",
    "caption": "Each box is a component, coloured by its kind. Each arrow is an edge of the architecture: data or control moving from one part to another.",
    "nodes": [{"id": str(i), "label": comp_name[i], "group": comp_kind[i]} for i in linked],
    "edges": [{"from": str(f), "to": str(t)} for f, t, _ in edges],
})

run = "".join(f'<li><div class="dc-home-card"><div class="dc-card-body"><code>{esc(c)}</code><span>{esc(s)}</span></div></div></li>'
              for c, s in commands)
parts = table(["Component", "Kind", "What it does"], [(esc(n), f'<span class="dc-dim">{esc(words(k, ""))}</span>', esc(t)) for n, k, t in components])
safe = table(["Threats the model names", "Controls that answer them"], [(
    "<br>".join(esc(words(t, "threat")) for t in threats), "<br>".join(esc(words(c, "control")) for c in controls))])
qual = table(["Measure", "Bound"], [(esc(words(n, "measure")), str(t)) for n, t in measures])
swatches = "".join(
    f'<li><div class="dc-home-card"><div class="swatch" style="background:{h}"></div><div class="dc-card-body"><em class="fd-mono">--{esc(n)}</em><strong>{esc(h)}</strong></div></div></li>'
    for n, k, v, h in tokens if k == "color")
ratios = table(["Text", "On", "Contrast", "WCAG 2.2 needs"], [
    (f"--{esc(tokens[f][0])}", f"--{esc(tokens[b][0])}", f"{o / 1000:.2f}:1", f"{r / 1000:g}:1") for f, b, o, r in contrasts])
identity = table(["Element", "Text"], [(esc(words(k, "")), esc(t)) for k, t in elements if k not in ("palette", "mark")])

body = f"""<header class="dc-header"><div class="dc-topbar"><a class="dc-brand" href="./"><span class="fd-head">{esc(name)}</span><span class="fd-mono">by {esc(element['name'])}</span></a>
<nav class="dc-social" aria-label="Community"><a href="{esc(LINKS['source'])}" aria-label="{esc(name)} on GitHub">{icon_git}</a></nav>
<a class="dc-cta" href="../" aria-label="Try it in your browser"><svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4v16l13-8z"/></svg><span>Try it in your browser</span></a></div>
<nav class="dc-tabs" aria-label="On this page"><a href="#who">Who it is for</a><a href="#run">Run it</a><a href="#parts">How it works</a><a href="#safe">Security</a><a href="#quality">Quality</a><a href="#brand">Brand</a></nav><span class="fd-header-line"></span></header>
<main id="main" class="dc-home">
<p class="eyebrow fd-mono">// {esc(element['tagline'])}</p>
<h1>{esc(name)}</h1>
<p class="dc-summary">{esc(lead)}</p>
<p class="dc-body">{esc(element['productPromise'])} {esc(APP['promise']['detail'])}</p>
<div class="dc-actions"><a class="dc-cta" href="../">Try the route table</a><a class="dc-ghost" href="{esc(LINKS['learn'])}">Learn {esc(name)}</a><a class="dc-ghost" href="{esc(LINKS['source'])}">Source</a><a class="dc-ghost" href="{esc(LINKS['releases'])}">Releases</a></div>
{hero}
{section("who", "Context of use · ISO 9241-11", "Who it is for", f"Every user group of {name}, the goal each one comes for, and the tasks that get it done, in order. Each task is a feature with acceptance tests. The guides below speak for each group.", '<div data-content="cast"></div><ul class="dc-home-grid">' + "".join(who) + "</ul>")}
{section("run", "Command line", "Run it", f"What {name}'s command line answers, read from the features that call it.", '<ul class="dc-home-grid">' + run + "</ul>")}
{section("parts", "Architecture · ISO/IEC/IEEE 42010", "How it works", f"The components of {name} and the systems it works with.", wiring + parts)}
{section("safe", "Security · ISO/IEC 27034 and 27005", "How it is kept safe", f"{len(threats)} threats and the {len(controls)} controls that answer them. The threat model maps every threat to a control with evidence.", safe)}
{section("quality", "Quality · ISO/IEC 25010", "How it is measured", "Each quality measure and the bound it must stay within.", qual)}
{section("brand", "Brand · Wheeler, W3C design tokens, WCAG 2.2", "The brand it carries", f"Read from {element['name']} at the commit brand/foundation.lock pins. Every text colour meets its WCAG 2.2 contrast criterion, a theorem the Lean kernel checks.", identity + '<ul class="dc-home-grid">' + swatches + "</ul>" + ratios)}
</main>
<footer class="dc-foot"><div class="dc-foot-row"><nav aria-label="Footer"><a href="{esc(LINKS['foundation'])}">{esc(element['name'])}</a><a href="{esc(LINKS['prismDocs'])}">Prism docs</a><a href="{esc(LINKS['learn'])}">Prism-DevRel</a><a href="{esc(LINKS['source'])}">GitHub</a></nav><span>{esc(element['steward'])}</span></div>
<p class="dc-foot-note">Every line of this page is read from {esc(name)}'s Prism model in app/src, which the Lean kernel checks, by scripts/experience/site.py.</p></footer>"""

page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="dark"><meta name="theme-color" content="{esc(dict((t[0], t[3]) for t in tokens)['fd-bg'])}"><title>{esc(name)} · {esc(element['name'])}</title><meta name="description" content="{esc(lead)}"><link rel="icon" href="../favicon.svg" type="image/svg+xml"><link rel="stylesheet" href="{esc(LINKS['stylesheet'])}"><link rel="stylesheet" href="foundation.css"><style>.swatch{{height:72px;border-bottom:1px solid hsl(var(--fd-line))}} .dc-home section{{margin-top:72px}} .dc-card-body code{{display:block;font:15px/1.5 "JetBrains Mono",monospace;overflow-wrap:anywhere}}</style></head>
<body class="foundation dc-root"><a class="skip" href="#main">Skip to content</a>{body}</body></html>
"""
(SITE / "docs").mkdir(parents=True, exist_ok=True)
(SITE / "docs/index.html").write_text(page)
shutil.copyfile(ROOT / "brand/foundation.css", SITE / "docs/foundation.css")

# The app page's ways on (app.json "links", drawn by the frame under the
# description) must include the product home this writes.
index = SITE / "index.html"
if index.exists():
    nav = re.search(r'<nav class="head-nav"[^>]*>([\s\S]*?)</nav>', index.read_text())
    if not nav or 'href="./docs/"' not in nav.group(1):
        raise SystemExit('site: the app page does not link to the product home (add {"href": "./docs/"} to app.json links)')
print(f"site: docs/index.html with {len(who)} goals, {len(commands)} commands, {len(components)} components, "
      f"{len(threats)} threats, {len(measures)} measures, {len(tokens)} tokens")
