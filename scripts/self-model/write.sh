#!/usr/bin/env bash
# Regenerate UORust's self model from its sources, inside the locked SDK.
#
#   model/routes.toml        -> app/src/Routes.lex.tex (routes, routeOf, vectors),
#                               app/src/Guide.lex.tex (sections, examples) and the
#                               [[vector]] / [[workbench_vector]] rows of routes.toml
#   scripts/self-model/facets.py -> Architecture, Design, Features, Security, Quality
#   scripts/self-model/release.py -> Release (the targets of release.yml, its
#                               artifacts, the channels install.yml proves) and
#                               the README's release-target table
#   scripts/experience/experience.py -> Brand, Users, Foundation.Experience.V1.Model,
#                               brand/tokens.json and brand/foundation.css, read
#                               from uor.foundation (brand/foundation.lock) and
#                               the facets above
#
# then lock and format the LexLean project, run `prismpm check`, and rebind the
# application-model digest in app/src/System.lex.tex when the check reports
# PP2101 (the digest changes with every edit of the model). Run as
# `just self-model-write`.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
python3 -I scripts/self-model/routes.py "$root"
python3 -I scripts/self-model/facets.py "$root" "$root" >/dev/null
python3 -I scripts/self-model/release.py "$root" "$root" >/dev/null
python3 -I scripts/experience/authority.py "$root" --check >/dev/null
python3 -I scripts/experience/experience.py "$root" "$root"
(cd app && lexlean lock >/dev/null && lexlean fmt >/dev/null)
if ! out=$(prismpm --project app check 2>&1); then
  old=$(sed -n 's/.*binds application model "\(sha256:[0-9a-f]*\)".*/\1/p' <<<"$out")
  new=$(sed -n 's/.*imported application model is \(sha256:[0-9a-f]*\).*/\1/p' <<<"$out")
  if [[ -z $old || -z $new ]]; then
    printf '%s\n' "$out" >&2
    exit 1
  fi
  sed -i "s/$old/$new/g" app/src/System.lex.tex
  out=$(prismpm --project app check 2>&1) || { printf '%s\n' "$out" >&2; exit 1; }
  echo "self-model: rebound the system release to $new"
fi
printf '%s\n' "$out"
