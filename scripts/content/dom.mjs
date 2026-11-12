/**
 * The parts of a page each rule reads, as CSS selectors, shared by the pass
 * that applies the rules (enrich.mjs) and the gate that measures them
 * (check.mjs), so the two always agree on what counts.
 */

/** Text that is not prose: code, data, navigation and the standard's own parts. */
export const NOT_PROSE = 'pre, code, kbd, samp, table, nav, svg, script, style, template, button, label, .cs-data, .cs-guide, .cs-terms, .dc-meta, .dc-pager, .dc-crumbs, .dc-mnav, [data-prose="data"]';

/** Where a term's first use is not looked for: headings, links, code and the same chrome. */
export const NOT_TERMS = `${NOT_PROSE}, a, h1, h2, h3, h4, h5, h6, summary, dt`;

/** Blocks that fold when taller than the compact window. */
export const FOLDABLE = 'table, pre, ul, ol, dl';

/** Blocks the gate measures: the foldable ones, and paragraphs and figures, which cannot fold and must be written short. */
export const MEASURED = 'table, pre, ul, ol, dl, p, figure, blockquote';

/** Blocks that are already out of sight or are the standard's own. */
export const SET_ASIDE = '.cs-guide, .cs-fold, .cs-terms, .cs-data, nav, details:not([open])';
