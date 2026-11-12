"""Generate the route table and acceptance vectors of app/src/Routes.lex.tex
from model/routes.toml, and append the [[vector]] rows to routes.toml.

The responses are computed here by an independent oracle of the routing rule;
the compiled guest replays them under `prismpm verify`."""
import sys, tomllib, re

root = sys.argv[1]
src = open(f"{root}/model/routes.toml", "rb").read()
data = tomllib.loads(src.decode())
routes = data["route"]
by_fact = {r["rust_fact"]: r for r in routes}
# The request limit is derived from the table, not chosen: the bytes of a
# request that names every kind once, one per line. A longer request repeats a
# kind or names one the table does not. self_model_sm_01 holds the model's
# Routes::requestLimit and the router's REQUEST_LIMIT to this sum.
EVERY_KIND = "\n".join(r["rust_fact"] for r in routes).encode()
LIMIT = len(EVERY_KIND)

def answer(req: bytes) -> str:
    if len(req) == 0:
        return "error empty"
    if len(req) > LIMIT:
        return "error too-large"
    try:
        text = req.decode("utf-8")
    except UnicodeDecodeError:
        return "error invalid-utf8"
    lines = text.split("\n")
    first = None
    first_tag = None
    for tag in lines:
        if tag == "":
            return "error blank-fact"
        r = by_fact.get(tag)
        if r is None:
            return f"refused unknown-fact\nfact {tag}"
        if first is None:
            first, first_tag = r, tag
        elif r["id"] != first["id"]:
            return f"refused ambiguous\nfact {first_tag}\nfact {tag}"
    return f"route {first['id']}\nfact {first_tag}\nrecord {first['prism_record']}\nfacet {first['facet']}"

ROUTED = "Routed: every kind names this route. The lines give the route, the fact kind, the Prism record kind and its facet."
ERROR = "Not routed: the fact is refused or the request is invalid. The details name the reason; its fix is listed under refusals and errors."
OVERFLOW = "The answer is too large for the page. Route this fact from the command line."

def esc(text: str) -> str:
    out = []
    for b in text.encode():
        if b == 34: out.append('\\"')
        elif b == 92: out.append('\\\\')
        elif b == 10: out.append('\\n')
        elif b == 13: out.append('\\r')
        elif b == 9: out.append('\\t')
        elif b < 32: out.append('\\u%04x' % b)
        else: out.append(bytes([b]).decode('latin-1'))
    return ''.join(out).encode('latin-1').decode('utf-8')

def jstr(t): return '"' + esc(t) + '"'
def jarr(items): return '[' + ','.join(jstr(i) for i in items) + ']'
def result_json(op, status, message, rows, cols, cells, details, request, response, error):
    return ('{"version":1,"status":' + jstr(status) + ',"operation":' + jstr(op) + ',"message":' + jstr(message)
            + ',"rows":' + str(rows) + ',"columns":' + str(cols) + ',"cells":' + jarr(cells) + ',"details":' + jarr(details)
            + ',"request":' + jstr(request) + ',"response":' + jstr(response) + ',"error":' + jstr(error) + '}')

def framed_answer(op, payload: str) -> bytes:
    report = answer(payload.encode())
    lines = report.split("\n")
    if lines[0].split(" ")[0] == "route":
        js = result_json(op, "ok", ROUTED, len(lines), 1, lines, [], payload, report, "")
    else:
        code = lines[0].split(" ")[-1]
        js = result_json(op, "error", ERROR, 0, 0, [], lines, payload, report, code)
    b = js.encode()
    if len(b) > 8192:
        b = result_json(op, "error", ERROR, 0, 0, [], [OVERFLOW], "", "", "too-large").encode()
    return b

EXAMPLES = [
    ("error-enum", "Error enum", "An enum that implements Error becomes a typed failure in the behavior facet.", "error-enum"),
    ("unsafe-block", "Unsafe block", "An unsafe block becomes a security threat.", "unsafe-block"),
    ("same-route", "Two kinds, one route", "Kinds that name the same route agree.", "test-fn\ntest-fn"),
    ("ambiguous", "Ambiguous fact", "An enum that is both an error and a state machine is refused, not defaulted.", "error-enum\nstate-enum"),
    ("unknown", "Unknown kind", "A kind no route names is refused as an open claim.", "struct"),
]

cases = []
for r in routes:
    cases.append((f"route {r['id']}", r["rust_fact"].encode()))
cases += [
    ("same route twice", b"test-fn\ntest-fn"),
    ("error empty", b""),
    ("error too-large", b"a" * (LIMIT + 1)),
    ("every kind once, at the limit", EVERY_KIND),
    ("error invalid-utf8", b"\xff"),
    ("error blank-fact", b"pub-fn\n"),
    ("error blank-fact first", b"\npub-fn"),
    ("refused unknown-fact", b"struct"),
    ("refused unknown-fact after a known fact", b"pub-fn\nmacro-rules"),
    ("refused ambiguous", b"error-enum\nstate-enum"),
    ("refused ambiguous same record", b"test-fn\ndoctest"),
    ("refused ambiguous after agreement", b"fs-use\nfs-use\nnet-use"),
]
vectors = [(name, req, answer(req).encode()) for name, req in cases]
for eid, label, desc, value in EXAMPLES:
    vectors.append((f"workbench {eid}", ("ui-route\x1f" + value).encode(), framed_answer("route", value)))
vectors.append(("workbench raw", "ui-raw\x1fpub-fn".encode(), framed_answer("raw", "pub-fn")))

def lstr(s):
    assert '"' not in s and "\\" not in s
    return '"' + s + '"'

route_lit = "(nil Route)"
for i, r in reversed(list(enumerate(routes))):
    rec = (f"(record Route {{index := {i}, id := {lstr(r['id'])}, rustFact := {lstr(r['rust_fact'])}, "
           f"prismRecord := {lstr(r['prism_record'])}, facet := {lstr(r['facet'])}}})")
    route_lit = f"(cons {rec} {route_lit})"

route_of = "(ctor Option.none [Route])"
for i, r in reversed(list(enumerate(routes))):
    rec = (f"(record Route {{index := {i}, id := {lstr(r['id'])}, rustFact := {lstr(r['rust_fact'])}, "
           f"prismRecord := {lstr(r['prism_record'])}, facet := {lstr(r['facet'])}}})")
    route_of = f"(if (prim equal tag {lstr(r['rust_fact'])} : Bool) then (ctor Option.some [Route] {rec}) else {route_of})"

V = "Foundation.View.Text.V1.Model::AcceptanceVector"
vec_lit = f"(nil {V})"
for name, req, resp in reversed(vectors):
    rec = f"(record {V} {{request := 0x{req.hex()}, response := 0x{resp.hex()}}})"
    vec_lit = f"(cons {rec} {vec_lit})"

tpl = open(f"{root}/app/src/Routes.lex.tex").read()
tpl = re.sub(r"(name: routes,\n  parameters: \[\],\n  result: \(List Route\),\n  body: ).*\n", lambda m: m.group(1) + route_lit + "\n", tpl)
tpl = re.sub(r"(name: acceptanceVectors,\n  parameters: \[\],\n  result: \(List [^\n]*\),\n  body: ).*\n", lambda m: m.group(1) + vec_lit + "\n", tpl)
tpl = re.sub(r"(name: routeOf,\n  parameters: \[tag : String\],\n  result: \(Option Route\),\n  body: ).*\n", lambda m: m.group(1) + route_of + "\n", tpl)
tpl = re.sub(r"(name: requestLimit,\s*parameters: \[\],\s*result: Nat,\s*body: )\d+", lambda m: m.group(1) + str(LIMIT), tpl)
# A request of at most LIMIT bytes has at most LIMIT + 1 lines.
tpl = re.sub(r"\(prim split_exact text \"\\n\" \(\d+ : UInt32\)", lambda m: f'(prim split_exact text "\\n" ({LIMIT + 1} : UInt32)', tpl)
# The application's byte bounds are the same limit: the browser takes a request
# of at most LIMIT bytes, and the guest allocates one byte more, so the
# too-large vector reaches the guest and its answer is modeled.
tpl = re.sub(r"requestMaximum := \(\d+ : UInt32\)", f"requestMaximum := ({LIMIT} : UInt32)", tpl)
tpl = re.sub(r"guestAllocationMaximum := \(\d+ : UInt32\)", f"guestAllocationMaximum := ({LIMIT + 1} : UInt32)", tpl)
# The generated program's package version is the workspace's.
cargo = open(f"{root}/Cargo.toml", encoding="utf-8").read()
version = re.search(r'\[workspace\.package\][^\[]*?\nversion = "([^"]+)"', cargo).group(1)
tpl = re.sub(r'cargoVersion := "[^"]*"', f'cargoVersion := "{version}"', tpl)
open(f"{root}/app/src/Routes.lex.tex", "w").write(tpl)

def lstr2(t):
    return '"' + t.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'
E = "Foundation.View.Text.V3.Model::Example"
ex_lit = f"(nil {E})"
for eid, label, desc, value in reversed(EXAMPLES):
    resp = framed_answer("route", value).decode()
    expl = answer(value.encode()).split("\n")[0]
    rec = (f"(record {E} {{id := {lstr2(eid)}, label := {lstr2(label)}, description := {lstr2(desc)}, "
           f"values := (cons {lstr2(value)} (nil String)), expectedResponse := {lstr2(resp)}, explanation := {lstr2(expl)}}})")
    ex_lit = f"(cons {rec} {ex_lit})"
FACETS = [("arch", "Architecture facet"), ("features", "Features and journeys"), ("behavior", "Behavior facet"),
          ("design", "Design views"), ("sec", "Security facet"), ("qual", "Quality facet"), ("release", "Release facet")]
SEC = "Foundation.View.Text.V3.Model::Section"
sec_lit = f"(nil {SEC})"
for fid, title in reversed(FACETS):
    paras = [f"{r['rust_fact']}: {r['id']} to {r['prism_record']}. {r['note']}" for r in routes if r['facet'] == fid]
    pl = "(nil String)"
    for para in reversed(paras):
        pl = f"(cons {lstr2(para)} {pl})"
    sec_lit = f"(cons (record {SEC} {{id := {lstr2(fid)}, title := {lstr2(title)}, paragraphs := {pl}}}) {sec_lit})"
g = open(f"{root}/app/src/Guide.lex.tex").read()
g = re.sub(r"(name: examples,\n  parameters: \[\],\n  result: \(List [^\n]*\),\n  body: ).*\n", lambda m: m.group(1) + ex_lit + "\n", g)
g = re.sub(r"(name: sections,\n  parameters: \[\],\n  result: \(List [^\n]*\),\n  body: ).*\n", lambda m: m.group(1) + sec_lit + "\n", g)
g = re.sub(r'explanation := "The request is longer than \d+ bytes[^"]*"', f'explanation := "The request is longer than {LIMIT} bytes, the length of one that names every kind of the table once."', g)
open(f"{root}/app/src/Guide.lex.tex", "w").write(g)

def tq(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'

text = src.decode()
text = text.split("\n# Acceptance vectors")[0].rstrip("\n") + "\n"
text = re.sub(r"at most \d+\n# bytes", f"at most {LIMIT}\n# bytes", text)
text = re.sub(r"more than \d+ bytes  ", f"more than {LIMIT} bytes  ", text)
text += "\n# Acceptance vectors: generated from the rule above; the application replays\n"
text += "# them in order. `request_hex` is the request's bytes, `response` its answer.\n"
text += "# A [[workbench_vector]] is framed for the page's workbench (`ui-route` or `ui-raw`,\n"
text += "# a unit separator, then the request) and answers JSON; only the application replays it.\n"
for name, req, resp in vectors:
    table = "workbench_vector" if name.startswith("workbench") else "vector"
    text += f"\n[[{table}]]\ncase = {tq(name)}\nrequest_hex = {tq(req.hex())}\nresponse = {tq(resp.decode())}\n"
open(f"{root}/model/routes.toml", "w").write(text)
print(len(routes), "routes,", len(vectors), "vectors")
