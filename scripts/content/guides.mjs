/**
 * The guides: one persona (Cooper, The Inmates Are Running the Asylum, 1999)
 * for each stakeholder of the model (ISO 9241-11 user groups), drawn as a
 * character so a reader recognizes whose page they are on. A guide is a
 * small faceted prism that splits a beam of light into the brand spectrum.
 * Its colour is the stakeholder's place in that spectrum, its prop is the
 * tool of the stakeholder's work, and model/content.json gives its name and
 * its words at each stage of the developer journey.
 */

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const PROPS = {
  terminal: '<rect x="2" y="4" width="20" height="16" rx="3"/><path d="m6 10 3 2-3 2M11 15h5"/>',
  compass: '<circle cx="12" cy="12" r="9"/><path d="m15.5 8.5-2 5-5 2 2-5z"/>',
  magnifier: '<circle cx="10" cy="10" r="6.5"/><path d="m15 15 6 6"/>',
  quill: '<path d="M20 3C11 4 6 9 4 20M20 3c-1 6-5 10-11 11M8 13l4 4"/>',
  wrench: '<path d="M14.5 5.5a4 4 0 0 0 4.9 5L12 18a2.1 2.1 0 0 1-3-3l7.5-7.4a4 4 0 0 0-2-2.1z"/>',
  shield: '<path d="M12 2 4 5v6c0 5 3.4 9.3 8 11 4.6-1.7 8-6 8-11V5z"/><path d="m9 12 2 2 4-4"/>',
};

const ACCESSORIES = {
  glasses: '<g class="cs-g-line"><circle cx="49" cy="62" r="8.5"/><circle cx="71" cy="62" r="8.5"/><path d="M57.5 61h5M40.5 60l-6-3M79.5 60l6-3"/></g>',
  beanie: '<path d="M36 30c4-14 44-14 48 0z" class="cs-g-hat"/><rect x="33" y="27" width="54" height="7" rx="3.5" class="cs-g-hat-band"/><circle cx="60" cy="12" r="5" class="cs-g-hat"/>',
  headset: '<g class="cs-g-line"><path d="M30 56c0-24 60-24 60 0"/><path d="M90 60v8c0 6-6 9-14 9"/></g><rect x="25" y="52" width="9" height="16" rx="4" class="cs-g-dark"/><rect x="86" y="52" width="9" height="16" rx="4" class="cs-g-dark"/><circle cx="75" cy="77" r="2.5" class="cs-g-dark"/>',
  sparkle: '<path d="M90 14l2.5 6 6 2.5-6 2.5-2.5 6-2.5-6-6-2.5 6-2.5z" class="cs-g-spark"/><path d="M24 20l1.5 3.5 3.5 1.5-3.5 1.5-1.5 3.5-1.5-3.5-3.5-1.5 3.5-1.5z" class="cs-g-spark"/>',
  goggles: '<rect x="37" y="40" width="46" height="13" rx="6.5" class="cs-g-dark"/><circle cx="49" cy="46.5" r="4.5" class="cs-g-lens"/><circle cx="71" cy="46.5" r="4.5" class="cs-g-lens"/>',
  none: '',
};

const hsl = (c) => {
  const m = c.match(/hsl\(([\d.]+)\s+([\d.]+)%\s+([\d.]+)%\)/);
  return m ? { h: +m[1], s: +m[2], l: +m[3] } : { h: 0, s: 0, l: 60 };
};
const tone = ({ h, s }, l) => `hsl(${h} ${s}% ${l}%)`;

let serial = 0;

/** The character, as an inline SVG (decorative: the guide card names it in text). */
export function avatar(persona, colour, palette, size = 96) {
  const c = hsl(colour);
  const id = `cs-g${++serial}`;
  const fan = palette.map((p, i) => {
    const y = 34 + (i * 56) / Math.max(1, palette.length - 1);
    return `<path d="M88 58L120 ${y.toFixed(1)}" stroke="${p}" stroke-width="2.4" stroke-linecap="round" opacity=".75"/>`;
  }).join('');
  const prop = PROPS[persona.prop] ?? PROPS.compass;
  const acc = ACCESSORIES[persona.accessory ?? 'none'] ?? '';
  return `<svg class="cs-avatar" viewBox="0 0 120 120" width="${size}" height="${size}" aria-hidden="true" focusable="false" xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="${id}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${tone(c, Math.min(82, c.l + 14))}"/><stop offset="1" stop-color="${tone(c, Math.max(30, c.l - 16))}"/></linearGradient></defs>`
    + `<path d="M0 64L32 58" stroke="hsl(0 0% 98%)" stroke-width="2.4" stroke-linecap="round" opacity=".8"/>${fan}`
    + `<ellipse cx="60" cy="108" rx="30" ry="5" class="cs-g-shadow"/>`
    + `<path d="M60 14L92 32V76L60 98L28 76V32Z" fill="url(#${id})"/>`
    + `<path d="M60 14L92 32L60 48L28 32Z" fill="hsl(0 0% 100% / .28)"/><path d="M60 48L92 32V76L60 98Z" fill="hsl(0 0% 0% / .18)"/><path d="M60 14L92 32V76L60 98L28 76V32Z" fill="none" stroke="hsl(0 0% 100% / .45)" stroke-width="1.5" stroke-linejoin="round"/>`
    + `<ellipse cx="49" cy="62" rx="6.5" ry="7.5" fill="hsl(0 0% 99%)"/><ellipse cx="71" cy="62" rx="6.5" ry="7.5" fill="hsl(0 0% 99%)"/><circle cx="51" cy="63.5" r="3.4" class="cs-g-dark"/><circle cx="73" cy="63.5" r="3.4" class="cs-g-dark"/><circle cx="52.2" cy="62" r="1.1" fill="hsl(0 0% 100%)"/><circle cx="74.2" cy="62" r="1.1" fill="hsl(0 0% 100%)"/>`
    + `<ellipse cx="41" cy="73" rx="4" ry="2.4" fill="hsl(350 90% 75% / .55)"/><ellipse cx="79" cy="73" rx="4" ry="2.4" fill="hsl(350 90% 75% / .55)"/><path d="M53 76q7 6 14 0" fill="none" stroke="hsl(0 0% 8%)" stroke-width="2.2" stroke-linecap="round"/>`
    + `<ellipse cx="47" cy="100" rx="7" ry="3.6" class="cs-g-dark"/><ellipse cx="73" cy="100" rx="7" ry="3.6" class="cs-g-dark"/>`
    + acc
    + `<path d="M92 70q8 2 10 8" fill="none" stroke="${tone(c, Math.max(30, c.l - 16))}" stroke-width="4" stroke-linecap="round"/><g transform="translate(94 76) scale(1.05)" class="cs-g-prop"><rect x="-2" y="-2" width="28" height="28" rx="8" class="cs-g-chip"/><g fill="none" stroke="hsl(0 0% 98%)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${prop}</g></g>`
    + '</svg>';
}

/** The persona whose page this is: an explicit audience, else the first rule of model/content.json that matches. */
export function personaFor(M, page) {
  if (page.audience) {
    const p = M.personas.find((x) => x.id === page.audience || x.stakeholder === page.audience);
    if (p) return p;
  }
  for (const r of M.content.audience) {
    if (r.path && !new RegExp(r.path).test(page.rel)) continue;
    if (r.diataxis && r.diataxis !== page.diataxis) continue;
    if (r.journey && r.journey !== page.journey) continue;
    return M.personas.find((p) => p.id === r.persona);
  }
  return null;
}

/** Each persona's colour: its stakeholder's place among the model's stakeholders, along the spectrum. */
export function colourOf(M, persona, palette) {
  const i = M.stakeholders.findIndex((s) => s.id === persona.stakeholder);
  return palette[Math.round((i * (palette.length - 1)) / Math.max(1, M.stakeholders.length - 1))];
}

/** The card that opens a page: who it is for, what they ask here, and how to read it. */
export function card(M, persona, page, palette) {
  const stage = page.journey ?? M.content.defaultStage;
  const says = persona.asks[stage] ?? persona.asks[M.content.defaultStage];
  const how = page.diataxis ? M.content.reading?.[page.diataxis] : M.content.reading?.home;
  return `<aside class="cs-guide" data-persona="${esc(persona.id)}" aria-label="Your guide on this page">${avatar(persona, colourOf(M, persona, palette), palette)}<div class="cs-guide-text"><p class="cs-guide-who"><strong>${esc(persona.name)}</strong> <span>${esc(persona.stakeholderName)}</span></p><p class="cs-guide-says">“${esc(says)}”</p>${how ? `<p class="cs-guide-how">${esc(how)}</p>` : ''}</div></aside>`;
}

/** Every guide at once, for a home page: who the docs are for. */
export function cast(M, palette, hrefOf = () => null) {
  return `<ul class="cs-cast">${M.personas.map((p) => {
    const href = hrefOf(p);
    const inner = `${avatar(p, colourOf(M, p, palette), palette, 88)}<strong>${esc(p.name)}</strong><span class="cs-cast-role">${esc(p.stakeholderName)}</span><span class="cs-cast-need">${esc(p.need)}</span>`;
    return `<li class="cs-cast-member" data-persona="${esc(p.id)}">${href ? `<a href="${esc(href)}">${inner}</a>` : inner}</li>`;
  }).join('')}</ul>`;
}
