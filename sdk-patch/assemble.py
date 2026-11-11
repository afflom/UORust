#!/usr/bin/env python3
"""Publish a patched PrismPM SDK image without rebuilding the SDK.

The image is the pinned SDK candidate's own layers plus two appended layers:
  1. the patched `prismpm` binary and the SDK inventory that names its digest;
  2. the terminal metadata layer (inventory.json, standards.lock) in exactly
     the form `prismpm lock update` reads (strict ustar, root, 0755/0444).
Every base layer is mounted or copied by digest from the source repository,
so nothing else in the SDK changes. Usage:

  assemble.py layer <arch> <binary> <base-ref> <out-dir>      # per architecture
  assemble.py index <target> <base-ref> <out-dir>...          # after both arches

The token for the target registry comes from GHCR_TOKEN (user GHCR_USER).
"""
import base64, gzip, hashlib, io, json, os, sys, urllib.error, urllib.request

REGISTRY = "ghcr.io"
OCI_INDEX = "application/vnd.oci.image.index.v1+json"
OCI_MANIFEST = "application/vnd.oci.image.manifest.v1+json"
OCI_LAYER = "application/vnd.oci.image.layer.v1.tar+gzip"
OCI_CONFIG = "application/vnd.oci.image.config.v1+json"
INVENTORY = "opt/prismpm/share/inventory.json"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return (json.dumps(value, separators=(",", ":"), sort_keys=True, ensure_ascii=False) + "\n").encode()


class Repo:
    def __init__(self, name, actions):
        self.name = name
        url = f"https://{REGISTRY}/token?scope=repository:{name}:{actions}&service={REGISTRY}"
        headers = {}
        if "push" in actions:
            cred = f"{os.environ['GHCR_USER']}:{os.environ['GHCR_TOKEN']}".encode()
            headers["Authorization"] = "Basic " + base64.b64encode(cred).decode()
        self.token = json.load(urllib.request.urlopen(urllib.request.Request(url, headers=headers)))["token"]

    def request(self, method, path, data=None, headers=None, absolute=False):
        url = path if absolute else f"https://{REGISTRY}/v2/{self.name}/{path}"
        headers = dict(headers or {}, Authorization="Bearer " + self.token)
        return urllib.request.urlopen(urllib.request.Request(url, data=data, method=method, headers=headers))

    def get(self, path, accept="*/*"):
        return self.request("GET", path, headers={"Accept": accept}).read()

    def has_blob(self, digest):
        try:
            self.request("HEAD", f"blobs/{digest}")
            return True
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return False
            raise

    def push_blob(self, digest, data):
        if self.has_blob(digest):
            return
        location = self.request("POST", "blobs/uploads/", data=b"").headers["Location"]
        if location.startswith("/"):
            location = f"https://{REGISTRY}{location}"
        sep = "&" if "?" in location else "?"
        self.request("PUT", f"{location}{sep}digest={digest}", data=data,
                     headers={"Content-Type": "application/octet-stream"}, absolute=True)

    def copy_blob(self, source, digest):
        if self.has_blob(digest):
            return
        try:
            response = self.request("POST", f"blobs/uploads/?mount={digest}&from={source.name}", data=b"")
            if response.status == 201:
                return
        except urllib.error.HTTPError:
            pass
        data = source.get(f"blobs/{digest}")
        assert sha(data) == digest.split(":")[1], digest
        self.push_blob(digest, data)

    def put_manifest(self, reference, media_type, body):
        self.request("PUT", f"manifests/{reference}", data=body, headers={"Content-Type": media_type})


def header(template, name, mode, size):
    """A ustar header in the base metadata layer's exact field encoding."""
    h = bytearray(template)
    h[0:100] = name.encode().ljust(100, b"\0")
    h[100:108] = f"{mode:07o}\0".encode()
    h[124:136] = f"{size:011o}\0".encode()
    h[148:156] = b"        "
    h[148:156] = f"{sum(h):06o}\0 ".encode()
    return bytes(h)


def entry(template, name, mode, data):
    pad = (-len(data)) % 512
    return header(template, name, mode, len(data)) + data + b"\0" * pad


def finish(body):
    body += b"\0" * 1024
    return body + b"\0" * ((-len(body)) % 10240)


def gz(data):
    out = io.BytesIO()
    with gzip.GzipFile(fileobj=out, mode="wb", mtime=0, compresslevel=9) as f:
        f.write(data)
    return out.getvalue()


def layer(arch, binary_path, base_ref, out):
    source_name, digest = base_ref.split("@")
    source = Repo(source_name.split("/", 1)[1], "pull")
    index = json.loads(source.get(f"manifests/{digest}", OCI_INDEX))
    row = next(m for m in index["manifests"] if m["platform"]["architecture"] == arch)
    manifest = json.loads(source.get(f"manifests/{row['digest']}", OCI_MANIFEST))
    config = json.loads(source.get(f"blobs/{manifest['config']['digest']}"))
    meta = gzip.decompress(source.get(f"blobs/{manifest['layers'][-1]['digest']}"))
    # Parse the base terminal metadata layer.
    entries, at = [], 0
    while meta[at:at + 512].strip(b"\0"):
        h = meta[at:at + 512]
        size = int(h[124:136].strip(b"\0 ") or b"0", 8)
        name = h[0:100].rstrip(b"\0").decode()
        entries.append((h, name, meta[at + 512: at + 512 + size]))
        at += 512 + size + ((-size) % 512)
    file_template = next(h for h, n, _ in entries if n == INVENTORY)
    files = {n: d for _, n, d in entries}
    binary = open(binary_path, "rb").read()
    inventory = json.loads(files[INVENTORY])
    binary_digest = sha(binary)
    hits = 0
    for artifact in inventory["artifacts"]:
        if artifact["id"] == "prismpm":
            artifact["digest"] = "sha256:" + binary_digest
            hits += 1
    for command in inventory["commands"]:
        if command["command"] == "prismpm":
            command["sha256"] = binary_digest
            hits += 1
    assert hits == 2, hits
    inventory_bytes = canonical(inventory)
    inventory_sha = f"{sha(inventory_bytes)}  /{INVENTORY}\n".encode()
    # Layer 1: the binary and the installed inventory beside it.
    tar1 = finish(entry(file_template, "usr/local/bin/prismpm", 0o755, binary)
                  + entry(file_template, INVENTORY, 0o444, inventory_bytes)
                  + entry(file_template, "opt/prismpm/share/inventory.sha256", 0o444, inventory_sha))
    # Layer 2: the terminal metadata layer, the base one with the new inventory.
    body = b""
    for h, name, data in entries:
        if name == INVENTORY:
            body += entry(file_template, name, 0o444, inventory_bytes)
        elif h[156:157] == b"5":
            body += h
        else:
            body += entry(file_template, name, 0o444, data)
    tar2 = finish(body)
    layers = []
    for tar in (tar1, tar2):
        blob = gz(tar)
        layers.append({"tar": tar, "blob": blob, "diff_id": "sha256:" + sha(tar), "digest": "sha256:" + sha(blob)})
    config["rootfs"]["diff_ids"] += [l["diff_id"] for l in layers]
    config["history"] += [
        {"created": "1970-01-01T00:00:00Z", "created_by": "prismpm patched binary and inventory", "comment": "uorust sdk-patch"},
        {"created": "1970-01-01T00:00:00Z", "created_by": "terminal SDK metadata", "comment": "uorust sdk-patch"},
    ]
    config_bytes = json.dumps(config, separators=(",", ":")).encode()
    manifest["config"] = {"mediaType": OCI_CONFIG, "digest": "sha256:" + sha(config_bytes), "size": len(config_bytes)}
    manifest["layers"] = manifest["layers"] + [
        {"mediaType": OCI_LAYER, "digest": l["digest"], "size": len(l["blob"])} for l in layers]
    os.makedirs(out, exist_ok=True)
    for l in layers:
        open(os.path.join(out, l["digest"].split(":")[1]), "wb").write(l["blob"])
    open(os.path.join(out, "config.json"), "wb").write(config_bytes)
    open(os.path.join(out, "manifest.json"), "wb").write(json.dumps(manifest, separators=(",", ":")).encode())
    json.dump({"arch": arch, "binary": binary_digest, "inventory": sha(inventory_bytes)}, open(os.path.join(out, "summary.json"), "w"))
    print(json.dumps({"arch": arch, "binary": binary_digest}))


def publish(target, base_ref, outs):
    source_name, _ = base_ref.split("@")
    source = Repo(source_name.split("/", 1)[1], "pull")
    dest_name = target.split("/", 1)[1].split(":")[0]
    tag = target.rsplit(":", 1)[1]
    dest = Repo(dest_name, f"push,pull&scope=repository:{source.name}:pull")
    rows = []
    for out in outs:
        summary = json.load(open(os.path.join(out, "summary.json")))
        manifest_bytes = open(os.path.join(out, "manifest.json"), "rb").read()
        manifest = json.loads(manifest_bytes)
        for layer_row in manifest["layers"]:
            local = os.path.join(out, layer_row["digest"].split(":")[1])
            if os.path.exists(local):
                dest.push_blob(layer_row["digest"], open(local, "rb").read())
            else:
                dest.copy_blob(source, layer_row["digest"])
        dest.push_blob(manifest["config"]["digest"], open(os.path.join(out, "config.json"), "rb").read())
        digest = "sha256:" + sha(manifest_bytes)
        dest.put_manifest(digest, OCI_MANIFEST, manifest_bytes)
        rows.append({"mediaType": OCI_MANIFEST, "digest": digest, "size": len(manifest_bytes),
                     "platform": {"architecture": summary["arch"], "os": "linux"}})
    rows.sort(key=lambda r: ["amd64", "arm64"].index(r["platform"]["architecture"]))
    index_bytes = json.dumps({"schemaVersion": 2, "mediaType": OCI_INDEX, "manifests": rows}, separators=(",", ":")).encode()
    dest.put_manifest(tag, OCI_INDEX, index_bytes)
    print(f"{REGISTRY}/{dest_name}@sha256:{sha(index_bytes)}")


if __name__ == "__main__":
    if sys.argv[1] == "layer":
        layer(*sys.argv[2:6])
    else:
        publish(sys.argv[2], sys.argv[3], sys.argv[4:])
