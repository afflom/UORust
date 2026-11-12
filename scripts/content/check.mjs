#!/usr/bin/env node
/**
 * The content gate. Opens every documentation page of the assembled site in
 * Chromium at the compact window class bootstrap/screens.mjs tests, measures
 * every rule of scripts/content/standard.json on the page as a reader gets
 * it, writes the measurements to SITE/content-report.json (and its Markdown
 * twin), and exits non-zero, so pages.yml deploys nothing, when one rule
 * fails on one page.
 *
 *   node scripts/content/check.mjs SITE [ROOT]
 *
 * ROOT (default: the repository holding this script) supplies
 * model/content.json (which directories hold documentation pages, the
 * personas and the glossary), app/src/Architecture.lex.tex (the stakeholders
 * a persona must be one of) and bootstrap/screens.mjs (the compact window).
 */
import { createServer } from 'node:http';
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { dirname, extname, join, normalize, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CODE, grade, longest, sentences, words } from './text.mjs';
import { load } from './model.mjs';
import { FOLDABLE, MEASURED, NOT_PROSE, NOT_TERMS, SET_ASIDE } from './dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SITE = process.argv[2] ?? 'site';
const ROOT = process.argv[3] ?? join(here, '../..');
const STANDARD = JSON.parse(readFileSync(join(here, 'standard.json'), 'utf8'));
const M = load(ROOT);
const rule = Object.fromEntries(STANDARD.rules.map((r) => [r.id, r]));

function walk(dir, out = []) {
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

const pages = walk(SITE).map((f) => relative(SITE, f).split('\\').join('/'))
  .filter((rel) => rel.endsWith('.html') && M.content.pages.some((d) => rel.startsWith(d)));
if (!pages.length) { console.error(`content: no documentation pages under ${SITE} in ${M.content.pages.join(', ')}`); process.exit(1); }

const types = { '.html': 'text/html', '.css': 'text/css', '.mjs': 'text/javascript', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.md': 'text/markdown' };
const server = createServer((request, response) => {
  let path = decodeURIComponent(new URL(request.url, 'http://x').pathname);
  if (path.endsWith('/')) path += 'index.html';
  const file = normalize(join(SITE, path));
  try {
    const body = readFileSync(file);
    response.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404);
    response.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}/`;

const playwright = await import('playwright').catch(() =>
  import(pathToFileURL(join(execSync('npm root -g').toString().trim(), 'playwright', 'index.mjs')).href));
const browser = await playwright.chromium.launch();
const context = await browser.newContext({ viewport: { width: M.compact.width, height: M.compact.height }, serviceWorkers: 'block' });
// A page may wear a shared stylesheet from another site; it is read as a reader gets it. Nothing else leaves the machine.
await context.route(/^https?:\/\/(?!127\.0\.0\.1)/, (route) => (['stylesheet', 'font'].includes(route.request().resourceType()) ? route.continue() : route.abort()));

/** Everything the rules read, taken from the rendered page. */
function observe({ terms, vague, personas, sel }) {
  const main = document.querySelector('main');
  const skip = sel.NOT_PROSE;
  // An element's prose: its text, with code spans as one word, nested lists and
  // data left out, and a line break wherever a block of its own begins.
  const blockOf = (n, root) => { for (let e = n.parentElement; e && e !== root; e = e.parentElement) if (getComputedStyle(e).display !== 'inline') return e; return root; };
  const proseOf = (el) => {
    let out = '';
    let last = null;
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, { acceptNode: (n) => (n !== el && n.nodeType === 1 && n.matches('ul, ol, dl, table, pre, figure, details, .cs-data, [data-prose="data"]') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT) });
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      if (n.nodeType === 1) { if (n.matches('code, kbd, samp')) { out += '\u2063'; } continue; }
      if (n.parentElement.closest('code, kbd, samp')) continue;
      const b = blockOf(n, el);
      if (last && b !== last) out += '\n';
      last = b;
      out += n.textContent;
    }
    return out.replace(/[ \t\r\f]+/g, ' ').trim();
  };
  const prose = [...main.querySelectorAll('p, li, dd, figcaption, blockquote, summary, h1, h2, h3, h4')]
    // Folded text is still read when it is opened, so it is measured too.
    .filter((el) => !el.closest(skip))
    .map((el) => ({ tag: el.tagName.toLowerCase(), text: proseOf(el), heading: /^H\d$/.test(el.tagName) }))
    .filter((b) => b.text);
  // CS-01
  const guide = main.querySelector('.cs-guide[data-persona]');
  // CS-04 / CS-05
  // A figure that holds a picture; a code listing in a figure is text, not an image.
  const figures = [...main.querySelectorAll('figure')].filter((f) => !f.closest('.cs-guide') && f.querySelector('svg, img, canvas, .cs-canvas'));
  const firstH2 = main.querySelector('h2');
  const leads = figures.some((f) => !firstH2 || (f.compareDocumentPosition(firstH2) & Node.DOCUMENT_POSITION_FOLLOWING));
  const figureFaults = figures.flatMap((f) => {
    const img = f.querySelector('[role="img"], img');
    const name = img && (img.getAttribute('aria-label') || img.getAttribute('alt') || img.querySelector('title')?.textContent || (img.getAttribute('aria-labelledby') && document.getElementById(img.getAttribute('aria-labelledby'))?.textContent));
    const faults = [];
    const id = f.id || f.querySelector('figcaption')?.textContent.slice(0, 50) || 'figure';
    if (!img || !name?.trim()) faults.push(`${id}: no image role with an accessible name`);
    if (!f.querySelector('figcaption')?.textContent.trim()) faults.push(`${id}: no caption`);
    if (!f.querySelector('.cs-data, table, [data-source]')) faults.push(`${id}: no data in text beside the picture`);
    return faults;
  });
  // CS-06
  const tall = [];
  const limit = innerHeight;
  for (const el of main.querySelectorAll(sel.MEASURED)) {
    if (el.closest(sel.SET_ASIDE) && !el.matches('.cs-figure')) continue;
    // A block inside another measured block is judged with it.
    if (el.parentElement.closest(sel.FOLDABLE)) continue;
    // A figure is judged by its picture: its data folds.
    if (el.matches('figure') && el.querySelector('.cs-canvas')) { const c = el.querySelector('.cs-canvas').getBoundingClientRect().height; if (c > limit) tall.push({ what: 'figure', height: Math.round(c), near: el.querySelector('figcaption')?.textContent.slice(0, 60) ?? '' }); continue; }
    if (el.matches('figure, blockquote') && el.querySelector(sel.FOLDABLE)) continue;
    const h = el.getBoundingClientRect().height;
    if (h > limit) tall.push({ what: `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}${el.className ? '.' + String(el.className).split(' ')[0] : ''}`, height: Math.round(h), near: (el.closest('section, details')?.querySelector('h2, h3, summary')?.textContent ?? el.textContent).trim().slice(0, 60) });
  }
  // CS-07
  const walker = document.createTreeWalker(main, NodeFilter.SHOW_TEXT, { acceptNode: (n) => (n.parentElement.closest(sel.NOT_TERMS) && !n.parentElement.closest('a.cs-term') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT) });
  const texts = [];
  for (let n; (n = walker.nextNode());) texts.push(n);
  const unmarked = [];
  for (const t of terms) {
    const re = new RegExp(`(?<![\\p{L}\\p{N}_-])(?:${t.match.join('|')})(?![\\p{L}\\p{N}_])`, t.case ? 'u' : 'iu');
    const first = texts.find((n) => re.test(n.textContent));
    if (first && !first.parentElement.closest('a.cs-term')) unmarked.push(t.term);
  }
  // CS-08
  const h1s = document.querySelectorAll('h1').length;
  const levels = [...main.querySelectorAll('h1, h2, h3, h4, h5, h6')].filter((h) => !h.closest('.cs-guide')).map((h) => ({ level: +h.tagName[1], text: h.textContent.trim() }));
  const skipped = levels.filter((h, i) => i > 0 && h.level > levels[i - 1].level + 1).map((h) => h.text);
  // CS-09
  const vagueLinks = [...main.querySelectorAll('a[href]')].map((a) => a.textContent.trim().toLowerCase()).filter((t) => vague.includes(t));
  // CS-11
  const ld = [...document.querySelectorAll('script[type="application/ld+json"]')].map((s) => { try { return JSON.parse(s.textContent); } catch { return { invalid: true }; } });
  const twin = document.querySelector('link[rel="alternate"][type="text/markdown"]')?.getAttribute('href');
  return {
    title: document.querySelector('h1')?.textContent.trim(),
    summary: document.querySelector('meta[name="description"]')?.content ?? '',
    persona: guide?.dataset.persona ?? null, knownPersona: guide ? personas.includes(guide.dataset.persona) : false,
    figures: figures.length, leads, figureFaults, tall, unmarked, h1s, levels, skipped, vagueLinks, ld, twin,
    canonical: document.querySelector('link[rel="canonical"]')?.href ?? null,
    prose,
  };
}

const llms = existsSync(join(SITE, 'llms.txt')) ? readFileSync(join(SITE, 'llms.txt'), 'utf8') : '';
const titleCase = (h) => {
  const ws = h.split(/\s+/).slice(1).filter((w) => /^[A-Z][a-z]{3,}$/.test(w) && !M.proper.has(w));
  return ws.length >= 2;
};
const fillerRe = new RegExp(`\\b(${STANDARD.fillers.map((f) => f.replace(/ /g, '\\s+')).join('|')})\\b`, 'gi');
const report = [];
const page = await context.newPage();
for (const rel of pages) {
  await page.goto(`${origin}${rel}`, { waitUntil: 'load' });
  await page.evaluate(() => document.fonts.ready.then(() => true));
  const o = await page.evaluate(observe, { terms: M.terms, vague: STANDARD.vagueLinks, personas: M.personas.map((p) => p.id), sel: { FOLDABLE, MEASURED, NOT_PROSE, NOT_TERMS, SET_ASIDE } });
  const faults = [];
  const add = (id, detail) => faults.push({ rule: id, detail });
  if (!o.persona) add('CS-01', 'no guide card');
  else if (!o.knownPersona) add('CS-01', `persona "${o.persona}" is not one of the model's stakeholders`);
  const g = grade(o.summary);
  if (!o.summary) add('CS-02', 'no summary');
  else if (g > rule['CS-02'].bound.max) add('CS-02', `summary reads at grade ${g.toFixed(1)}: "${o.summary}"`);
  let worst = { words: 0, sentence: '' };
  for (const b of o.prose) {
    if (b.heading) continue;
    const l = longest(b.text);
    if (l.words > rule['CS-03'].bound.max) add('CS-03', `${l.words} words: "${l.sentence.replace(new RegExp(CODE, 'g'), '`…`')}"`);
    if (l.words > worst.words) worst = l;
  }
  if (!o.figures) add('CS-04', 'no figure');
  else if (!o.leads) add('CS-04', 'no figure before the first section');
  for (const f of o.figureFaults) add('CS-05', f);
  for (const t of o.tall) add('CS-06', `${t.what} is ${t.height}px tall, over ${M.compact.height}px (${t.near})`);
  for (const t of o.unmarked) add('CS-07', `first "${t}" is not marked as a defined term`);
  if (o.h1s !== 1) add('CS-08', `${o.h1s} h1 elements`);
  for (const s of o.skipped) add('CS-08', `heading "${s}" skips a level`);
  for (const h of o.levels) if (titleCase(h.text)) add('CS-08', `heading "${h.text}" is in title case`);
  for (const v of o.vagueLinks) add('CS-09', `link text "${v}"`);
  for (const b of o.prose) for (const m of b.text.matchAll(fillerRe)) add('CS-10', `"${m[0]}" in "${b.text.slice(0, 80)}"`);
  const art = o.ld.find((d) => d['@type'] === 'TechArticle');
  if (!art) add('CS-11', 'no Schema.org TechArticle');
  else for (const k of ['headline', 'description', 'audience', 'learningResourceType', 'educationalLevel']) if (!art[k]) add('CS-11', `TechArticle has no ${k}`);
  if (!o.twin) add('CS-11', 'no Markdown twin');
  else if (!existsSync(join(SITE, dirname(rel), o.twin.split('#')[0]))) add('CS-11', `Markdown twin ${o.twin} is missing`);
  const url = rel.replace(/index\.html$/, '');
  if (!llms) add('CS-11', 'the site has no llms.txt');
  else if (!llms.includes(url ? `/${url}` : '/docs/') && !llms.includes(`/${rel.replace(/\.html$/, '.md')}`) && !llms.includes(`/${rel.replace(/index\.html$/, 'index.md')}`)) add('CS-11', 'not listed in llms.txt');
  const proseWords = o.prose.filter((b) => !b.heading).flatMap((b) => sentences(b.text)).flatMap(words).length;
  report.push({ page: rel, title: o.title, persona: o.persona, summaryGrade: +g.toFixed(2), longestSentence: worst.words, proseWords, figures: o.figures, tallestOpenBlock: Math.max(0, ...o.tall.map((t) => t.height)), faults });
}
await browser.close();
server.close();

const byRule = Object.fromEntries(STANDARD.rules.map((r) => [r.id, report.reduce((n, p) => n + p.faults.filter((f) => f.rule === r.id).length, 0)]));
const out = {
  spec: 'prism/content-report/1',
  standard: STANDARD.spec,
  compact: M.compact,
  pages: report.length,
  faults: Object.values(byRule).reduce((a, b) => a + b, 0),
  byRule,
  measures: {
    worstSummaryGrade: Math.max(...report.map((p) => p.summaryGrade)),
    longestSentence: Math.max(...report.map((p) => p.longestSentence)),
    pagesWithoutFigures: report.filter((p) => !p.figures).length,
    figures: report.reduce((n, p) => n + p.figures, 0),
  },
  report,
};
writeFileSync(join(SITE, 'content-report.json'), `${JSON.stringify(out, null, 2)}\n`);
const md = [`# Content report`, '', `${out.pages} pages measured against [the content standard](https://github.com/afflom/prism-devrel/blob/main/scripts/content/standard.json) at the ${M.compact.width}×${M.compact.height} compact window. ${out.faults} faults.`, '', '| Rule | Title | Faults |', '|---|---|---|', ...STANDARD.rules.map((r) => `| ${r.id} | ${r.title} | ${byRule[r.id]} |`), ''].join('\n');
writeFileSync(join(SITE, 'content-report.md'), md);
const verbose = process.env.CONTENT_VERBOSE !== undefined;
for (const p of report) {
  if (!p.faults.length) continue;
  console.error(`${p.page}: ${p.faults.length} faults`);
  const shown = verbose ? p.faults : p.faults.slice(0, 6);
  for (const f of shown) console.error(`  ${f.rule} ${f.detail}`);
  if (shown.length < p.faults.length) console.error(`  … ${p.faults.length - shown.length} more (CONTENT_VERBOSE=1 lists them)`);
}
console.log(`content: ${out.pages} pages, ${out.faults} faults (${Object.entries(byRule).map(([k, v]) => `${k} ${v}`).join(', ')})`);
process.exit(out.faults ? 1 : 0);
