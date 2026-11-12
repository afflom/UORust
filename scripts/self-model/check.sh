#!/usr/bin/env bash
# Fail when the committed self model is not what its generators write: copy the
# sources to a scratch directory, regenerate there, format with lexlean, and
# compare every generated file byte for byte. Run as `just self-model-check`
# (part of `just vv`).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/model" "$work/scripts"
cp "$root/model/routes.toml" "$work/model/"
cp "$root/Cargo.toml" "$work/"
cp -r "$root/scripts/self-model" "$work/scripts/"
tar -C "$root" --exclude=app/.lexlean --exclude=app/.prism --exclude=app/target --exclude=app/.lake -cf - app | tar -C "$work" -xf -
python3 -I "$work/scripts/self-model/routes.py" "$work"
python3 -I "$work/scripts/self-model/facets.py" "$work" "$root" >/dev/null
python3 -I "$work/scripts/self-model/release.py" "$work" "$root" >/dev/null
python3 -I "$root/scripts/experience/authority.py" "$root" --check
python3 -I "$root/scripts/experience/experience.py" "$work" "$root" >/dev/null
(cd "$work/app" && lexlean fmt >/dev/null)
status=0
for file in model/routes.toml app/src/Routes.lex.tex app/src/Guide.lex.tex app/src/Architecture.lex.tex \
  app/src/Design.lex.tex app/src/Features.lex.tex app/src/Security.lex.tex app/src/Quality.lex.tex \
  app/src/Release.lex.tex README.md \
  app/src/Brand.lex.tex app/src/Users.lex.tex app/src/Foundation/Experience/V1/Model.lex.tex \
  app/language/prism.arch/entries/brand-design-and-context-of-use.toml \
  app/language/prism.arch/entries/uorust-brand-identity.toml app/language/prism.arch/entries/uorust-context-of-use.toml \
  brand/tokens.json brand/foundation.css; do
  if ! cmp -s "$root/$file" "$work/$file"; then
    echo "self-model: $file is not what scripts/self-model writes; run just self-model-write" >&2
    diff -u "$root/$file" "$work/$file" | head -20 >&2 || true
    status=1
  fi
done
[[ $status -eq 0 ]] && echo "self-model: every generated file matches its generator"
exit $status
