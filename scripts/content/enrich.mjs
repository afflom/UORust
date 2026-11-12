#!/usr/bin/env node
/**
 * Applies the parts of the content standard (standard.json) a machine can
 * apply, to every documentation page of an assembled site, in Chromium with
 * the page's own stylesheets and scripts off:
 *
 *   figures   each <figure class="cs-figure"> spec a generator wrote becomes
 *             the drawn figure, its caption and its data (figures.mjs)  CS-04/05
 *   guide     a guide card for the page's persona after the lead      CS-01
 *   terms     the first use of each glossary term links to its plain
 *             definition in "Words on this page"                       CS-07
 *   folds     every block taller than the compact window goes behind a
 *             disclosure that says what it holds and how much          CS-06
 *   data      Schema.org TechArticle JSON-LD, a Markdown twin when the
 *             generator wrote none, and an llms.txt entry              CS-11
 *
 *   node scripts/content/enrich.mjs SITE [ROOT]
 *
 * Run it after the pages are written and wrapped, before they are precached;
 * scripts/content/check.mjs then measures the result. It refuses a page that
 * already carries a guide card, so it never runs twice over one page.
 */
import { createServer } from 'node:http';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { dirname, extname, join, normalize, posix, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { load } from './model.mjs';
import { figure, spectrum } from './figures.mjs';
import { avatar, card, cast, colourOf, personaFor } from './guides.mjs';
import { FOLDABLE, NOT_TERMS, SET_ASIDE } from './dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SITE = process.argv[2] ?? 'site';
const ROOT = process.argv[3] ?? join(here, '../..');
const M = load(ROOT);
const PALETTE = spectrum(readFileSync(join(ROOT, 'brand/foundation.css'), 'utf8'));
const CSS = M.content.css;

function walk(dir, out = []) {
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const slug = (s) => s.toLowerCase().normalize('NFKD').replace(/\p{M}/gu, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

const pages = walk(SITE).map((f) => relative(SITE, f).split('\\').join('/'))
  .filter((rel) => rel.endsWith('.html') && M.content.pages.some((d) => rel.startsWith(d)));

mkdirSync(dirname(join(SITE, CSS)), { recursive: true });
copyFileSync(join(here, 'content.css'), join(SITE, CSS));

const types = { '.html': 'text/html', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png' };
const server = createServer((request, response) => {
  let path = decodeURIComponent(new URL(request.url, 'http://x').pathname);
  if (path.endsWith('/')) path += 'index.html';
  try {
    const file = normalize(join(SITE, path));
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
const context = await browser.newContext({ viewport: { width: M.compact.width, height: M.compact.height }, javaScriptEnabled: false, serviceWorkers: 'block' });
// A page may wear a shared stylesheet from another site; it is read as a reader gets it. Nothing else leaves the machine.
await context.route(/^https?:\/\/(?!127\.0\.0\.1)/, (route) => (['stylesheet', 'font'].includes(route.request().resourceType()) ? route.continue() : route.abort()));
const tab = await context.newPage();

const app = existsSync(join(SITE, 'app.json')) ? JSON.parse(readFileSync(join(SITE, 'app.json'), 'utf8')) : {};
const live = (app.live ?? '').replace(/\/?$/, '/');
let llms = existsSync(join(SITE, 'llms.txt')) ? readFileSync(join(SITE, 'llms.txt'), 'utf8') : null;
const terms = M.terms.map((t) => ({ id: `term-${slug(t.term)}`, term: t.term, plain: t.plain, match: t.match, case: !!t.case }));

/** In the page: mark the first use of each term, and list the terms used. */
function markTerms(list, sel) {
  const main = document.querySelector('main');
  const reject = sel.NOT_TERMS;
  const used = [];
  for (const t of list) {
    const re = new RegExp(`(?<![\\p{L}\\p{N}_-])(?:${t.match.join('|')})(?![\\p{L}\\p{N}_])`, t.case ? 'u' : 'iu');
    const walker = document.createTreeWalker(main, NodeFilter.SHOW_TEXT);
    for (let n; (n = walker.nextNode());) {
      if (n.parentElement.closest(reject)) continue;
      const m = re.exec(n.textContent);
      if (!m) continue;
      const after = n.splitText(m.index);
      after.splitText(m[0].length);
      const a = document.createElement('a');
      a.className = 'cs-term';
      a.href = `#${t.id}`;
      a.title = t.plain;
      a.textContent = after.textContent;
      after.replaceWith(a);
      used.push(t);
      break;
    }
  }
  if (!used.length) return 0;
  const box = document.createElement('details');
  box.className = 'cs-terms';
  box.id = 'words';
  box.innerHTML = `<summary>Words on this page <span>${used.length}</span></summary><dl>${used.map((t) => `<dt id="${t.id}">${t.term}</dt><dd>${t.plain.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</dd>`).join('')}</dl>`;
  const pager = main.querySelector('.dc-pager');
  if (pager) pager.before(box); else main.append(box);
  return used.length;
}

/** In the page: put every block taller than the window behind a disclosure. */
function fold(sel) {
  const main = document.querySelector('main');
  const limit = innerHeight;
  let folded = 0;
  const heading = (el) => {
    for (let n = el; n && n !== main; n = n.parentElement) {
      for (let p = n.previousElementSibling; p; p = p.previousElementSibling) if (/^H[2-4]$/.test(p.tagName)) return p.textContent.trim();
    }
    return document.querySelector('h1')?.textContent.trim() ?? '';
  };
  const blocks = [...main.querySelectorAll(sel.FOLDABLE)].filter((el) => !el.closest(`${sel.SET_ASIDE}, figure, .cs-cast`) && !el.parentElement.closest(sel.FOLDABLE));
  for (const el of blocks) {
    if (el.getBoundingClientRect().height <= limit) continue;
    const target = el.parentElement.matches('div.table, .table-wrap, .scroll') ? el.parentElement : el;
    let what;
    if (el.tagName === 'TABLE') { const n = el.tBodies[0]?.rows.length ?? el.rows.length; what = `${n} rows`; }
    else if (el.tagName === 'PRE') what = `${el.textContent.trimEnd().split('\n').length} lines`;
    else if (el.tagName === 'DL') what = `${el.querySelectorAll(':scope > dt').length} entries`;
    else what = `${el.querySelectorAll(':scope > li').length} items`;
    const box = document.createElement('details');
    box.className = 'cs-fold';
    const title = el.querySelector('caption')?.textContent.trim() || heading(el);
    box.innerHTML = `<summary><span class="cs-fold-what">${title.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</span> <span class="cs-fold-n">${what}</span></summary>`;
    target.replaceWith(box);
    box.append(target);
    folded++;
  }
  return folded;
}

function twinOf(rel, html) {
  // The page's own main, as Markdown: headings, paragraphs, lists, tables and code.
  const main = html.match(/<main[^>]*>([\s\S]*?)<\/main>/)?.[1] ?? '';
  const t = (s) => s.replace(/<[^>]+>/g, '').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&').replace(/\s+/g, ' ').trim();
  const md = main
    .replace(/<(script|style|svg)[\s\S]*?<\/\1>/g, '')
    .replace(/<h([1-4])[^>]*>([\s\S]*?)<\/h\1>/g, (a, l, s) => `\n\n${'#'.repeat(+l)} ${t(s)}\n\n`)
    .replace(/<pre[^>]*>([\s\S]*?)<\/pre>/g, (a, s) => `\n\n\`\`\`\n${t(s.replace(/<br>/g, '\n')).replace(/ {2,}/g, ' ')}\n\`\`\`\n\n`)
    .replace(/<li[^>]*>([\s\S]*?)<\/li>/g, (a, s) => `\n- ${t(s)}`)
    .replace(/<tr[^>]*>([\s\S]*?)<\/tr>/g, (a, s) => `\n| ${[...s.matchAll(/<t[hd][^>]*>([\s\S]*?)<\/t[hd]>/g)].map((c) => t(c[1]).replace(/\|/g, '\\|')).join(' | ')} |`)
    .replace(/<p[^>]*>([\s\S]*?)<\/p>/g, (a, s) => `\n\n${t(s)}\n\n`)
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&')
    .replace(/\n{3,}/g, '\n\n').trim();
  // The first row of each table is its header.
  return `${md.replace(/((?:^|\n)\| [^\n]*\|)(\n\|)/g, (a, head, next) => `${head}\n|${' --- |'.repeat(head.split(' | ').length)}${next}`)}\n`;
}

let figuresDrawn = 0;
let cards = 0;
let marked = 0;
let folds = 0;
for (const rel of pages) {
  const file = join(SITE, rel);
  let html = readFileSync(file, 'utf8');
  if (html.includes('class="cs-guide"')) throw new Error(`${rel}: already enriched; write the pages again first`);
  const head = (name) => html.match(new RegExp(`<meta name="${name}" content="([^"]*)"`))?.[1];
  const info = { rel, diataxis: head('diataxis'), journey: head('developer-journey'), audience: head('audience') };
  const persona = personaFor(M, info);
  if (!persona) throw new Error(`${rel}: no persona in model/content.json audience matches this page`);
  const root = '../'.repeat(rel.split('/').length - 1);
  const cssHref = posix.relative(posix.dirname(rel), CSS);
  // Figures are drawn here, in Node, from the spec each generator wrote.
  html = html.replace(/<figure class="cs-figure"[^>]*>\s*<script type="application\/vnd\.prism\.figure\+json">([\s\S]*?)<\/script>\s*<\/figure>/g, (all, json) => {
    figuresDrawn++;
    const spec = JSON.parse(json);
    // A step may be illustrated by a guide: "persona:<id>" draws that character.
    const draw = { ...spec, steps: spec.steps?.map((s) => {
      const g = typeof s.icon === 'string' && s.icon.startsWith('persona:') ? M.personas.find((p) => p.id === s.icon.slice(8)) : null;
      return g ? { ...s, icon: avatar(g, colourOf(M, g, PALETTE), PALETTE).replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, ''), sub: `${g.name}: ${s.sub}` } : s;
    }) };
    if (!draw.steps) delete draw.steps;
    return figure(draw, PALETTE).replace(/<script type="application\/vnd\.prism\.figure\+json">[\s\S]*?<\/script>/, `<script type="application/vnd.prism.figure+json">${json}</script>`);
  });
  // The cast of guides, where a page asks for it: each links to where that guide starts.
  html = html.replace(/<div data-content="cast"><\/div>/g, () => cast(M, PALETTE, (p) => (p.start ? posix.relative(posix.dirname(rel), p.start) + '/' : null)));
  if (!/<link rel="alternate" type="text\/markdown"/.test(html)) {
    const twin = rel.replace(/index\.html$/, 'index.md').replace(/\.html$/, '.md');
    writeFileSync(join(SITE, twin), twinOf(rel, html));
    html = html.replace('</head>', `<link rel="alternate" type="text/markdown" href="${posix.basename(twin)}"></head>`);
  }
  const summary = html.match(/<meta name="description" content="([^"]*)"/)?.[1] ?? '';
  const title = (html.match(/<h1[^>]*>([\s\S]*?)<\/h1>/)?.[1] ?? '').replace(/<[^>]+>/g, '').trim();
  const url = `${live}${rel.replace(/index\.html$/, '')}`;
  const ld = {
    '@context': 'https://schema.org',
    '@type': 'TechArticle',
    headline: title,
    description: summary.replace(/&quot;/g, '"').replace(/&amp;/g, '&'),
    url,
    inLanguage: 'en',
    audience: { '@type': 'Audience', audienceType: persona.stakeholderName, description: persona.characteristics },
    learningResourceType: info.diataxis ?? M.content.defaultType,
    educationalLevel: info.journey ?? M.content.defaultStage,
    isPartOf: { '@type': 'WebSite', name: M.content.site, url: live },
    publisher: { '@type': 'Organization', name: 'UOR Foundation', url: 'https://uor.foundation/' },
  };
  html = html.replace('</head>', `<link rel="stylesheet" href="${cssHref}"><script type="application/ld+json">${JSON.stringify(ld).replace(/</g, '\\u003c')}</script></head>`);
  writeFileSync(file, html);
  await tab.goto(`${origin}${rel}`, { waitUntil: 'load' });
  await tab.evaluate(() => document.fonts.ready.then(() => true));
  const guide = card(M, persona, info, PALETTE);
  const r = await tab.evaluate(([guide, list, markTermsSrc, foldSrc, sel]) => {
    const markTerms = new Function(`return (${markTermsSrc})`)();
    const fold = new Function(`return (${foldSrc})`)();
    const main = document.querySelector('main');
    const h1 = main.querySelector('h1');
    // After the lead: the first paragraph after the title.
    let at = h1;
    for (let n = h1.nextElementSibling; n; n = n.nextElementSibling) { if (n.tagName === 'P') { at = n; break; } if (/^H[2-6]$/.test(n.tagName) || n.tagName === 'SECTION') break; }
    at.insertAdjacentHTML('afterend', guide);
    const terms = markTerms(list, sel);
    const folds = fold(sel);
    return { html: `<!doctype html>\n${document.documentElement.outerHTML}\n`, terms, folds };
  }, [guide, terms, markTerms.toString(), fold.toString(), { FOLDABLE, NOT_TERMS, SET_ASIDE }]);
  marked += r.terms;
  folds += r.folds;
  cards++;
  writeFileSync(file, r.html);
  if (llms !== null && !llms.includes(url) && !llms.includes(`${live}${rel.replace(/index\.html$/, 'index.md')}`)) {
    llms = `${llms.trimEnd()}\n\n## ${M.content.site} pages\n\n- [${title}](${url}): ${ld.description}\n`;
  }
}
if (llms !== null) writeFileSync(join(SITE, 'llms.txt'), llms);
else {
  writeFileSync(join(SITE, 'llms.txt'), `# ${M.content.site}\n\n${pages.map((rel) => `- [${rel}](${live}${rel.replace(/index\.html$/, '')})`).join('\n')}\n`);
}
await browser.close();
server.close();
console.log(`enrich: ${cards} pages, ${figuresDrawn} figures drawn, ${marked} terms marked, ${folds} long blocks folded`);
