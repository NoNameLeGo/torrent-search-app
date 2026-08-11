'use strict';

/**
 * Shared mirror-retry helper.
 *
 * Many providers define a list of mirror domains and try them in parallel,
 * returning the first non-empty result set.  This function encapsulates that
 * pattern so individual providers only need to supply their per-domain logic.
 *
 * @param {Array<() => Promise<{results: Array, error: string|null}>>} attempts
 *        One async factory per mirror; each resolves to { results, error }.
 * @param {string} [name='unknown'] — human-readable provider name for the
 *        aggregate error message when all mirrors fail.
 * @returns {Promise<{results: Array, error: string|null}>}
 *          The result from the first successful mirror, or the aggregated error.
 */
async function runMirrors(attempts, name = 'unknown') {
  const settled = await Promise.allSettled(attempts.map((fn) => fn()));
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value && s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `${name} unreachable (${errs.join('; ')})` };
}

module.exports = { runMirrors };
