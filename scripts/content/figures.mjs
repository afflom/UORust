/**
 * Figures for documentation pages, drawn from data. A generator in any
 * language writes a declarative figure spec into the page:
 *
 *   <figure class="cs-figure"><script type="application/vnd.prism.figure+json">{...}</script></figure>
 *
 * and scripts/content/enrich.mjs replaces it with the inline SVG this module
 * draws, a caption, and the same data as a table (WCAG 2.2 SC 1.1.1: the
 * picture's text alternative; machines read the spec itself, which stays in
 * the page). Every figure is drawn twice where the shape needs it, wide for a
 * page column and tall for the compact window class, and CSS shows one.
 *
 * Kinds:
 *   flow      {steps: [{label, sub?, icon?}]}                 a process, in order
 *   graph     {nodes: [{id, label, sub?, group?, layer?}], edges: [{from, to, label?}]}
 *             layered left to right when wide; an arc diagram when tall
 *   bars      {series?: [name], rows: [{label, value | values, note?}], unit?, bound?: {value, label}}
 *   quadrant  {x: [low, high], y: [low, high], cells: [{label, sub?, x: 0|1, y: 0|1, href?}]}
 *
 * Colours are the page's own tokens (--fd-*) and the brand spectrum, which
 * the caller passes in from brand/foundation.css; nothing here picks a colour.
 */

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** The width of a wide figure: the docs article column, so text draws at its own size. */
const WIDE = 680;

/** Average advance of Space Grotesk and JetBrains Mono, in em: how labels are wrapped. */
const ADVANCE = { sans: 0.56, mono: 0.6 };
const textWidth = (s, size, face = 'sans') => String(s).length * size * ADVANCE[face];

export function wrap(label, chars) {
  const out = [];
  let line = '';
  for (const w of String(label).split(/\s+/)) {
    if (!line) line = w;
    else if ((line + ' ' + w).length <= chars) line += ' ' + w;
    else { out.push(line); line = w; }
  }
  if (line) out.push(line);
  return out.flatMap((l) => (l.length > chars * 1.6 ? l.match(new RegExp(`.{1,${chars}}`, 'g')) : [l]));
}

let serial = 0;
const uid = (p) => `${p}${++serial}`;

function textBlock(lines, x, y, size, cls, anchor = 'middle', lead = 1.3) {
  const top = y - ((lines.length - 1) * size * lead) / 2;
  return lines.map((l, i) => `<text x="${x.toFixed(1)}" y="${(top + i * size * lead).toFixed(1)}" class="${cls}" text-anchor="${anchor}" dominant-baseline="middle">${esc(l)}</text>`).join('');
}

function svg(w, h, title, desc, body, cls) {
  const t = uid('cs-t');
  const d = uid('cs-d');
  return `<svg class="cs-svg ${cls}" viewBox="0 0 ${Math.ceil(w)} ${Math.ceil(h)}" role="img" aria-labelledby="${t} ${d}" xmlns="http://www.w3.org/2000/svg"><title id="${t}">${esc(title)}</title><desc id="${d}">${esc(desc)}</desc>${body}</svg>`;
}

const arrowDef = (id) => `<defs><marker id="${id}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" class="cs-arrowhead"/></marker></defs>`;

/** A process in order: cards joined by arrows, a row when wide and a column when tall. */
function flow(spec, palette) {
  const steps = spec.steps;
  const n = steps.length;
  const accent = (i) => palette[Math.round((i * (palette.length - 1)) / Math.max(1, n - 1))];
  const card = (s, i, x, y, w, h, chars) => {
    const lines = wrap(s.label, chars);
    const sub = s.sub ? wrap(s.sub, Math.round(chars * 1.1)) : [];
    // Each drawing of an icon gets its own ids, so a hidden copy never owns a gradient the shown one uses.
    const own = uid('i');
    const art = s.icon ? s.icon.replace(/\bid="([^"]+)"/g, `id="$1-${own}"`).replace(/url\(#([^)]+)\)/g, `url(#$1-${own})`) : '';
    const icon = s.icon ? `<g transform="translate(${x + w / 2 - 22} ${y + 12}) scale(${44 / 120})">${art}</g>` : '';
    const top = y + (s.icon ? 60 : 30);
    const labelH = lines.length * 17;
    return `<g class="cs-node"><rect x="${x}" y="${y}" width="${w}" height="${h}" rx="12" class="cs-card"/><rect x="${x}" y="${y}" width="${w}" height="4" rx="2" fill="${accent(i)}"/><text x="${x + 12}" y="${y + 20}" class="cs-step" dominant-baseline="middle">${String(i + 1).padStart(2, '0')}</text>${icon}${textBlock(lines, x + w / 2, top + labelH / 2 + 4, 14, 'cs-label')}${sub.length ? textBlock(sub, x + w / 2, top + labelH + 10 + (sub.length * 15) / 2, 11.5, 'cs-sub') : ''}</g>`;
  };
  const chars = 15;
  const maxLines = Math.max(...steps.map((s) => wrap(s.label, chars).length * 17 + (s.sub ? wrap(s.sub, Math.round(chars * 1.1)).length * 15 + 10 : 0)));
  const iconH = steps.some((s) => s.icon) ? 48 : 0;
  // Wide: one row.
  const gap = 28;
  const W = WIDE;
  const cw = (W - gap * (n - 1)) / n;
  const ch = 54 + iconH + maxLines;
  const aw = uid('cs-a');
  let wide = arrowDef(aw);
  steps.forEach((s, i) => {
    const x = i * (cw + gap);
    wide += card(s, i, x, 0, cw, ch, Math.max(8, Math.floor((cw - 16) / (14 * ADVANCE.sans))));
    if (i < n - 1) wide += `<line x1="${x + cw + 3}" y1="${ch / 2}" x2="${x + cw + gap - 3}" y2="${ch / 2}" class="cs-edge" marker-end="url(#${aw})"/>`;
  });
  // Tall: one column.
  const TW = 340;
  const at = uid('cs-a');
  let tall = arrowDef(at);
  let y = 0;
  steps.forEach((s, i) => {
    const lines = wrap(s.label, 30).length * 17 + (s.sub ? wrap(s.sub, 34).length * 15 + 10 : 0);
    const h = 54 + iconH + lines;
    tall += card(s, i, 0, y, TW, h, 30);
    y += h;
    if (i < n - 1) { tall += `<line x1="${TW / 2}" y1="${y + 3}" x2="${TW / 2}" y2="${y + 25}" class="cs-edge" marker-end="url(#${at})"/>`; y += 28; }
  });
  const desc = steps.map((s, i) => `${i + 1}. ${s.label}${s.sub ? `: ${s.sub}` : ''}`).join(' ');
  return [svg(W, ch + 2, spec.title, desc, wide, 'cs-wide'), svg(TW, y + 2, spec.title, desc, tall, 'cs-tall')].join('');
}

/** Layers by longest path from the sources, with edges that close a cycle set aside. */
function layering(nodes, edges) {
  const ids = nodes.map((n) => n.id);
  const out = Object.fromEntries(ids.map((i) => [i, []]));
  for (const e of edges) if (out[e.from] && out[e.to] !== undefined) out[e.from].push(e.to);
  const state = {};
  const back = new Set();
  const visit = (v) => {
    state[v] = 1;
    for (const w of out[v]) {
      if (state[w] === 1) back.add(`${v}\u0000${w}`);
      else if (!state[w]) visit(w);
    }
    state[v] = 2;
  };
  for (const v of ids) if (!state[v]) visit(v);
  const layer = Object.fromEntries(nodes.map((n) => [n.id, n.layer ?? null]));
  if (nodes.every((n) => n.layer !== undefined)) return { layer, back };
  const order = [];
  const seen = new Set();
  const topo = (v) => { if (seen.has(v)) return; seen.add(v); for (const w of out[v]) if (!back.has(`${v}\u0000${w}`)) topo(w); order.unshift(v); };
  for (const v of ids) topo(v);
  for (const v of order) {
    if (layer[v] === null) layer[v] = 0;
    for (const w of out[v]) if (!back.has(`${v}\u0000${w}`) && nodes.find((n) => n.id === w).layer === undefined) layer[w] = Math.max(layer[w] ?? 0, layer[v] + 1);
  }
  return { layer, back };
}

/** Nodes and edges: layered columns when wide, an arc diagram when tall. */
function graph(spec, palette) {
  const { nodes, edges } = spec;
  const { layer } = layering(nodes, edges);
  const L = Math.max(...Object.values(layer)) + 1;
  const cols = Array.from({ length: L }, (_, i) => nodes.filter((n) => layer[n.id] === i));
  // Barycentre sweeps keep crossings down.
  const pos = {};
  const index = () => cols.forEach((c) => c.forEach((n, i) => { pos[n.id] = i; }));
  index();
  for (let sweep = 0; sweep < 4; sweep++) {
    for (let l = 1; l < L; l++) {
      const bc = (n) => { const ps = edges.filter((e) => e.to === n.id && layer[e.from] === l - 1).map((e) => pos[e.from]); return ps.length ? ps.reduce((a, b) => a + b, 0) / ps.length : pos[n.id]; };
      cols[l].sort((a, b) => bc(a) - bc(b));
      index();
    }
    for (let l = L - 2; l >= 0; l--) {
      const bc = (n) => { const ps = edges.filter((e) => e.from === n.id && layer[e.to] === l + 1).map((e) => pos[e.to]); return ps.length ? ps.reduce((a, b) => a + b, 0) / ps.length : pos[n.id]; };
      cols[l].sort((a, b) => bc(a) - bc(b));
      index();
    }
  }
  const groups = [...new Set(nodes.map((n) => n.group).filter(Boolean))];
  const colour = (n) => (n.group ? palette[Math.round((groups.indexOf(n.group) * (palette.length - 1)) / Math.max(1, groups.length - 1))] : palette[0]);
  const desc = `${nodes.length} nodes and ${edges.length} links. ${edges.map((e) => `${nodes.find((n) => n.id === e.from)?.label} ${e.label ? e.label : 'to'} ${nodes.find((n) => n.id === e.to)?.label}`).join('; ')}.`;

  // Wide: columns.
  // A node is at least as wide as its longest word, so no label spills out;
  // when the columns need more than the article width, the drawing widens
  // and scales down to fit, keeping each label inside its box.
  const word = Math.max(...nodes.flatMap((n) => String(n.label).split(/\s+/).map((w) => textWidth(w, 13) + 24)));
  const minGap = 56;
  const W = Math.max(WIDE, L * word + minGap * (L - 1));
  const colGap = L > 1 ? Math.max(minGap, Math.min(120, (W - L * Math.max(140, word)) / (L - 1))) : 0;
  const nw = (W - colGap * (L - 1)) / L;
  const chars = Math.max(8, Math.floor((nw - 20) / (13 * ADVANCE.sans)));
  const nh = (n) => 22 + wrap(n.label, chars).length * 16 + (n.sub ? wrap(n.sub, Math.round(chars * 1.15)).length * 14 : 0);
  const loops = new Set(edges.filter((e) => e.from === e.to).map((e) => e.from));
  const rowGap = loops.size ? 44 : 16;
  const heights = cols.map((c) => c.reduce((s, n) => s + nh(n), 0) + rowGap * (c.length - 1));
  const back = edges.filter((e) => e.from !== e.to && layer[e.to] <= layer[e.from]);
  // Back edges arc over the top of the drawing, one lane each.
  const lanes = back.length ? 22 + back.length * 16 : 0;
  const top0 = lanes + (loops.size ? 56 : 0);
  const H = Math.max(...heights);
  const box = {};
  cols.forEach((c, l) => {
    let y = top0 + (H - heights[l]) / 2;
    for (const n of c) { box[n.id] = { x: l * (nw + colGap), y, w: nw, h: nh(n) }; y += nh(n) + rowGap; }
  });
  const aw = uid('cs-a');
  let wide = arrowDef(aw);
  const placed = [];
  const hits = (r) => [...Object.values(box), ...placed].some((b) => r.x < b.x + b.w && r.x + r.w > b.x && r.y < b.y + b.h && r.y + r.h > b.y);
  const bez = (p, t) => { const u = 1 - t; return [0, 1].map((k) => u * u * u * p[0][k] + 3 * u * u * t * p[1][k] + 3 * u * t * t * p[2][k] + t * t * t * p[3][k]); };
  const label = (p, text) => {
    const w = textWidth(text, 11) + 12;
    for (const t of [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8]) {
      const [x, y] = bez(p, t);
      const r = { x: x - w / 2, y: y - 9, w, h: 18 };
      if (!hits(r)) { placed.push(r); return; }
    }
    const [x, y] = bez(p, 0.5);
    placed.push({ x: x - w / 2, y: y - 9, w, h: 18 });
  };
  const texts = [];
  let lane = 0;
  for (const e of edges) {
    const a = box[e.from];
    const b = box[e.to];
    if (!a || !b) continue;
    let p;
    if (e.from === e.to) {
      const x = a.x + a.w - 30; const y = a.y;
      p = [[x, y], [x - 4, y - 34], [x + 26, y - 34], [x + 22, y - 2]];
    } else if (layer[e.to] > layer[e.from]) {
      const x1 = a.x + a.w; const y1 = a.y + a.h / 2; const x2 = b.x - 2; const y2 = b.y + b.h / 2;
      const k = (x2 - x1) / 2;
      p = [[x1, y1], [x1 + k, y1], [x2 - k, y2], [x2, y2]];
    } else {
      const top = lanes - 14 - lane++ * 16;
      const x1 = a.x + a.w * 0.62; const y1 = a.y; const x2 = b.x + b.w * 0.38; const y2 = b.y - 2;
      p = [[x1, y1], [x1, top - (y1 - top) * 0.34], [x2, top - (y2 - top) * 0.34], [x2, y2]];
    }
    wide += `<path d="M${p[0].join(' ')}C${p[1].join(' ')} ${p[2].join(' ')} ${p[3].join(' ')}" class="cs-edge${layer[e.to] > layer[e.from] ? '' : ' cs-back'}" marker-end="url(#${aw})"/>`;
    if (e.label && e.from === e.to) {
      // A loop's label sits on top of the loop, raised until it is clear.
      const w = textWidth(e.label, 11) + 12;
      let r = { x: p[0][0] + 11 - w / 2, y: p[0][1] - 46, w, h: 18 };
      while (hits(r)) r = { ...r, y: r.y - 20 };
      placed.push(r);
      texts.push(e.label);
    } else if (e.label) { label(p, e.label); texts.push(e.label); }
  }
  for (const n of nodes) {
    const b = box[n.id];
    const lines = wrap(n.label, chars);
    const sub = n.sub ? wrap(n.sub, Math.round(chars * 1.15)) : [];
    wide += `<g class="cs-node"><rect x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" rx="10" class="cs-card"/><rect x="${b.x}" y="${b.y + 8}" width="3" height="${b.h - 16}" rx="1.5" fill="${colour(n)}"/>${textBlock(lines, b.x + b.w / 2, b.y + 11 + (lines.length * 16) / 2, 13, 'cs-label')}${sub.length ? textBlock(sub, b.x + b.w / 2, b.y + 15 + lines.length * 16 + (sub.length * 14) / 2, 11, 'cs-sub') : ''}</g>`;
  }
  placed.forEach((r, i) => {
    wide += `<g class="cs-edge-label"><rect x="${r.x.toFixed(1)}" y="${r.y.toFixed(1)}" width="${r.w.toFixed(1)}" height="18" rx="9" class="cs-pill"/><text x="${(r.x + r.w / 2).toFixed(1)}" y="${(r.y + 9).toFixed(1)}" text-anchor="middle" dominant-baseline="middle" class="cs-sub">${esc(texts[i])}</text></g>`;
  });
  const minY = Math.min(0, ...placed.map((r) => r.y - 4));
  const maxX = Math.max(W, ...placed.map((r) => r.x + r.w + 2));
  const wideSvg = svg(maxX, top0 + H - minY + 4, spec.title, desc, `<g transform="translate(0 ${-minY})">${wide}</g>`, 'cs-wide');

  // Tall: an arc diagram, nodes in layer order down the left, links as arcs on the right.
  const TW = 340;
  const order = cols.flat();
  const nodeW = 210;
  const tchars = Math.floor((nodeW - 20) / (13 * ADVANCE.sans));
  const th = (n) => 18 + wrap(n.label, tchars).length * 16;
  let y = 0;
  const tb = {};
  for (const n of order) { tb[n.id] = { y, h: th(n) }; y += th(n) + 10; }
  const at = uid('cs-a');
  let tall = arrowDef(at);
  const rightX = nodeW + 4;
  for (const e of edges) {
    const a = tb[e.from]; const b = tb[e.to];
    if (!a || !b) continue;
    const y1 = a.y + a.h / 2; const y2 = b.y + b.h / 2;
    const r = Math.min(TW - rightX - 6, 14 + Math.abs(y2 - y1) * 0.35);
    tall += `<path d="M${rightX} ${y1 - 3}C${rightX + r} ${y1 - 3} ${rightX + r} ${y2 + 3} ${rightX + 4} ${y2 + 3}" class="cs-edge${y2 < y1 ? ' cs-back' : ''}" marker-end="url(#${at})"/>`;
  }
  for (const n of order) {
    const b = tb[n.id];
    const lines = wrap(n.label, tchars);
    tall += `<g class="cs-node"><rect x="0" y="${b.y}" width="${nodeW}" height="${b.h}" rx="10" class="cs-card"/><rect x="0" y="${b.y + 6}" width="3" height="${b.h - 12}" rx="1.5" fill="${colour(n)}"/>${textBlock(lines, 12, b.y + b.h / 2, 13, 'cs-label', 'start')}</g>`;
  }
  return wideSvg + svg(TW, y, spec.title, desc, tall, 'cs-tall');
}

/** Horizontal bars, stacked when there are series, with an optional bound line. */
function bars(spec, palette) {
  const series = spec.series ?? [spec.unit ?? 'value'];
  const rows = spec.rows.map((r) => ({ ...r, values: r.values ?? [r.value] }));
  const max = Math.max(spec.bound?.value ?? 0, ...rows.map((r) => r.values.reduce((a, b) => a + b, 0))) || 1;
  const W = 640;
  const labelW = Math.min(240, Math.max(...rows.map((r) => textWidth(r.label, 13))) + 12);
  const valueW = 64;
  const plotW = W - labelW - valueW;
  const rowH = 30;
  const legend = series.length > 1 ? 28 : 0;
  const colour = (i) => palette[Math.round((i * (palette.length - 1)) / Math.max(1, series.length - 1))];
  let body = '';
  if (legend) {
    let x = labelW;
    series.forEach((s, i) => { body += `<rect x="${x}" y="6" width="12" height="12" rx="3" fill="${colour(i)}"/><text x="${x + 18}" y="12" dominant-baseline="middle" class="cs-sub">${esc(s)}</text>`; x += 30 + textWidth(s, 11); });
  }
  rows.forEach((r, i) => {
    const y = legend + i * rowH;
    body += `<text x="${labelW - 10}" y="${y + rowH / 2}" text-anchor="end" dominant-baseline="middle" class="cs-label cs-left">${esc(r.label.length > 34 ? `${r.label.slice(0, 33)}…` : r.label)}</text>`;
    body += `<rect x="${labelW}" y="${y + 6}" width="${plotW}" height="${rowH - 12}" rx="4" class="cs-track"/>`;
    let x = labelW;
    r.values.forEach((v, k) => {
      const w = (v / max) * plotW;
      if (w > 0) body += `<rect x="${x.toFixed(1)}" y="${y + 6}" width="${Math.max(2, w).toFixed(1)}" height="${rowH - 12}" rx="4" fill="${colour(k)}"/>`;
      x += w;
    });
    const total = r.values.reduce((a, b) => a + b, 0);
    body += `<text x="${W - valueW + 8}" y="${y + rowH / 2}" dominant-baseline="middle" class="cs-value">${esc(r.note ?? String(total))}</text>`;
  });
  if (spec.bound) {
    const x = labelW + (spec.bound.value / max) * plotW;
    body += `<line x1="${x}" y1="${legend}" x2="${x}" y2="${legend + rows.length * rowH}" class="cs-bound"/><text x="${x}" y="${legend + rows.length * rowH + 12}" text-anchor="middle" class="cs-sub">${esc(spec.bound.label)}</text>`;
  }
  const H = legend + rows.length * rowH + (spec.bound ? 22 : 4);
  const desc = rows.map((r) => `${r.label}: ${series.length > 1 ? r.values.map((v, k) => `${v} ${series[k]}`).join(', ') : r.values[0]}${spec.unit && series.length === 1 ? ` ${spec.unit}` : ''}`).join('; ');
  return svg(W, H, spec.title, desc, body, 'cs-any');
}

/** Two axes and four cells, as in the Diátaxis compass. */
function quadrant(spec, palette) {
  const S = 520;
  const pad = 56;
  const cell = (S - pad * 2) / 2;
  let body = `<rect x="${pad}" y="${pad}" width="${cell * 2}" height="${cell * 2}" rx="16" class="cs-card"/><line x1="${pad + cell}" y1="${pad}" x2="${pad + cell}" y2="${pad + cell * 2}" class="cs-axis"/><line x1="${pad}" y1="${pad + cell}" x2="${pad + cell * 2}" y2="${pad + cell}" class="cs-axis"/>`;
  body += `<text x="${pad + cell / 2}" y="${S - 18}" text-anchor="middle" class="cs-sub">${esc(spec.x[0])}</text><text x="${pad + cell * 1.5}" y="${S - 18}" text-anchor="middle" class="cs-sub">${esc(spec.x[1])}</text>`;
  body += `<text transform="translate(22 ${pad + cell * 1.5}) rotate(-90)" text-anchor="middle" class="cs-sub">${esc(spec.y[0])}</text><text transform="translate(22 ${pad + cell / 2}) rotate(-90)" text-anchor="middle" class="cs-sub">${esc(spec.y[1])}</text>`;
  spec.cells.forEach((c, i) => {
    const x = pad + c.x * cell;
    const y = pad + (1 - c.y) * cell;
    const lines = wrap(c.label, 16);
    const sub = c.sub ? wrap(c.sub, 24) : [];
    const inner = `<rect x="${x + 10}" y="${y + 10}" width="${cell - 20}" height="${cell - 20}" rx="12" class="cs-cell"/><rect x="${x + 22}" y="${y + 22}" width="28" height="4" rx="2" fill="${palette[Math.round((i * (palette.length - 1)) / Math.max(1, spec.cells.length - 1))]}"/>${textBlock(lines, x + cell / 2, y + cell / 2 - 14, 20, 'cs-big')}${sub.length ? textBlock(sub, x + cell / 2, y + cell / 2 + 16 + sub.length * 7, 12, 'cs-sub') : ''}`;
    body += c.href ? `<a href="${esc(c.href)}">${inner}</a>` : inner;
  });
  const desc = spec.cells.map((c) => `${c.label} (${spec.x[c.x]}, ${spec.y[c.y]})${c.sub ? `: ${c.sub}` : ''}`).join('; ');
  return svg(S, S, spec.title, desc, body, 'cs-any cs-square');
}

/** The figure's data as a table: its text alternative and what a machine reads. */
export function table(spec) {
  const th = (cols) => `<thead><tr>${cols.map((c) => `<th scope="col">${esc(c)}</th>`).join('')}</tr></thead>`;
  const tr = (cells) => `<tr>${cells.map((c) => `<td>${esc(c ?? '')}</td>`).join('')}</tr>`;
  if (spec.kind === 'flow') return `<table>${th(['Step', 'What happens'])}<tbody>${spec.steps.map((s, i) => tr([`${i + 1}. ${s.label}`, s.sub])).join('')}</tbody></table>`;
  if (spec.kind === 'graph') {
    const label = (id) => spec.nodes.find((n) => n.id === id)?.label ?? id;
    return `<table>${th(['From', 'Link', 'To'])}<tbody>${spec.edges.map((e) => tr([label(e.from), e.label ?? '→', label(e.to)])).join('')}</tbody></table>`;
  }
  if (spec.kind === 'bars') {
    const series = spec.series ?? [spec.unit ?? 'Value'];
    return `<table>${th(['', ...series])}<tbody>${spec.rows.map((r) => tr([r.label, ...(r.values ?? [r.value]).map(String)])).join('')}</tbody></table>`;
  }
  if (spec.kind === 'quadrant') return `<table>${th(['', spec.x[0], spec.x[1]])}<tbody>${[1, 0].map((y) => `<tr><th scope="row">${esc(spec.y[y])}</th>${[0, 1].map((x) => `<td>${esc(spec.cells.filter((c) => c.x === x && c.y === y).map((c) => c.label).join(', '))}</td>`).join('')}</tr>`).join('')}</tbody></table>`;
  throw new Error(`figure kind ${spec.kind} has no table`);
}

const KINDS = { flow, graph, bars, quadrant };

/** The whole figure: the picture, its caption and its data. */
export function figure(spec, palette) {
  const draw = KINDS[spec.kind];
  if (!draw) throw new Error(`unknown figure kind "${spec.kind}"`);
  for (const k of ['title', 'caption']) if (!spec[k]) throw new Error(`figure "${spec.title ?? spec.kind}" has no ${k}`);
  const json = JSON.stringify(spec).replace(/</g, '\\u003c');
  return `<figure class="cs-figure cs-${spec.kind}"${spec.id ? ` id="${esc(spec.id)}"` : ''}><div class="cs-canvas">${draw(spec, palette)}</div><figcaption><strong>${esc(spec.title)}.</strong> ${esc(spec.caption)}</figcaption><details class="cs-data"><summary>The data in this figure</summary>${table(spec)}</details><script type="application/vnd.prism.figure+json">${json}</script></figure>`;
}

/** The brand spectrum's first period, from brand/foundation.css, as hsl() colours. */
export function spectrum(css) {
  const m = css.match(/spectrum: linear-gradient\(([^;]+)\)/);
  if (!m) throw new Error('brand/foundation.css has no spectrum');
  const stops = [...m[1].matchAll(/hsl\(([^)]+)\) ([\d.]+)%/g)].map((s) => ({ c: `hsl(${s[1]})`, at: +s[2] }));
  const period = stops.findIndex((s, i) => i > 0 && s.c === stops[0].c);
  return stops.slice(0, period > 0 ? period : stops.length).map((s) => s.c);
}
