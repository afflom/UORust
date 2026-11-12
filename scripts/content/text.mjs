/**
 * The text measures of the content standard (scripts/content/standard.json):
 * sentences, words, syllables and the Flesch-Kincaid grade of Kincaid,
 * Fishburne, Rogers and Chissom (1975). One module, so the gate
 * (check.mjs) and the writers that test their own copy measure alike.
 */

/** A code span, a command or an identifier reads as one word. */
export const CODE = '⁣';

/** Sentences of a block of prose: split after . ! ? or : before a capital, a digit or the end. */
export function sentences(text) {
  // A line break is a block boundary: it ends a sentence too.
  return String(text).split(/\n+/).flatMap(line);
}

function line(text) {
  const clean = String(text).replace(/\s+/g, ' ').trim();
  if (!clean) return [];
  // Abbreviations and dotted names do not end a sentence.
  const guarded = clean
    .replace(/\b(e\.g|i\.e|etc|vs|cf|Dr|Mr|Ms|No|Fig|Ed|ed|al)\./g, '$1․')
    .replace(/(\w)\.(?=\w)/g, '$1․');
  return guarded
    .split(/(?<=[.!?])\s+(?=[\p{Lu}\d"“(⁣])/u)
    .map((s) => s.replace(/․/g, '.').trim())
    .filter((s) => /[\p{L}\d⁣]/u.test(s));
}

/** Words of a sentence; each code span is one. */
export function words(sentence) {
  return String(sentence).split(/[\s—–/]+/).map((w) => w.replace(/^[^\p{L}\p{N}⁣]+|[^\p{L}\p{N}⁣]+$/gu, '')).filter(Boolean);
}

/** Syllables of an English word, by vowel groups (the usual approximation of the formula's authors). */
export function syllables(word) {
  const w = word.toLowerCase().replace(/[^a-z]/g, '');
  if (!w) return 1;
  if (w.length <= 3) return 1;
  const trimmed = w.replace(/(?:[^laeiouy]es|[^laeiouy]ed|[^laeiouy]e)$/, '').replace(/^y/, '');
  const groups = trimmed.match(/[aeiouy]{1,2}/g);
  return Math.max(1, groups ? groups.length : 1);
}

/** Flesch-Kincaid grade level of a passage. */
export function grade(text) {
  const ss = sentences(text);
  const ws = ss.flatMap(words);
  if (!ws.length) return 0;
  const syl = ws.reduce((n, w) => n + (w.includes(CODE) ? 1 : syllables(w)), 0);
  return 0.39 * (ws.length / ss.length) + 11.8 * (syl / ws.length) - 15.59;
}

/** Words in the longest sentence of a passage, and the sentence. */
export function longest(text) {
  let best = { words: 0, sentence: '' };
  for (const s of sentences(text)) {
    const n = words(s).length;
    if (n > best.words) best = { words: n, sentence: s };
  }
  return best;
}
