/**
 * What the content standard reads from the repository it runs in:
 *   model/content.json            documentation directories, personas, extra terms
 *   scripts/content/glossary.json the terms every Prism model's docs share
 *   app/src/Architecture.lex.tex  the stakeholders (a persona is one of them)
 *   bootstrap/screens.mjs         the compact window class every page is read at
 * Nothing here is a constant of its own: each value comes from one of those.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

export class ContentError extends Error {}
const refuse = (m) => { throw new ContentError(m); };

/** Stakeholder records of the model's Architecture module, by definition name. */
export function stakeholders(root) {
  const src = readFileSync(join(root, 'app/src/Architecture.lex.tex'), 'utf8');
  const out = [];
  for (const m of src.matchAll(/name: (stakeholder\w+),[\s\S]*?body: \(record Foundation\.Arch::Stakeholder \{index := (\d+), name := "((?:[^"\\]|\\.)*)", text := "((?:[^"\\]|\\.)*)"\}\)/g)) {
    out.push({ id: m[1], index: +m[2], name: m[3], text: m[4] });
  }
  if (!out.length) refuse('app/src/Architecture.lex.tex declares no stakeholders');
  return out;
}

/** The compact window class, as bootstrap/screens.mjs declares it. */
export function compact(root) {
  const src = readFileSync(join(root, 'bootstrap/screens.mjs'), 'utf8');
  const m = src.match(/name: "compact", width: (\d+), height: (\d+)/) ?? refuse('bootstrap/screens.mjs declares no compact window class');
  return { width: +m[1], height: +m[2] };
}

export function load(root) {
  const content = JSON.parse(readFileSync(join(root, 'model/content.json'), 'utf8'));
  const shared = JSON.parse(readFileSync(join(here, 'glossary.json'), 'utf8'));
  const holders = stakeholders(root);
  const byId = Object.fromEntries(holders.map((s) => [s.id, s]));
  for (const p of content.personas) if (!byId[p.stakeholder]) refuse(`model/content.json: persona ${p.id} names ${p.stakeholder}, which app/src/Architecture.lex.tex does not declare`);
  for (const s of holders) if (!content.personas.some((p) => p.stakeholder === s.id)) refuse(`model/content.json: stakeholder ${s.id} ("${s.name}") has no persona`);
  const personas = content.personas.map((p) => ({ ...p, stakeholderName: byId[p.stakeholder].name, characteristics: byId[p.stakeholder].text }));
  const terms = [...shared.terms, ...(content.terms ?? [])].map((t) => ({ ...t, match: t.match ?? [t.term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')] }));
  const proper = new Set([...shared.proper, ...(content.proper ?? []), ...personas.map((p) => p.name)]);
  for (const t of terms) for (const w of t.term.split(/\s+/)) if (/^[A-Z]/.test(w)) proper.add(w);
  return { content, personas, terms, proper, compact: compact(root), stakeholders: holders };
}
