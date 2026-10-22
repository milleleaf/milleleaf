#!/usr/bin/env node
// compare — check that two deck layers describe the same nodes: same paths, geometry within
// 1px, same paragraph text and run formatting. Used for the HTML round-trip test:
//   layer → `mleaf render` → HTML → html2mleaf → layer   must come back unchanged.
//
//   node compare.mjs a.mleaf.json b.mleaf.json [--warn-geometry]
//
// --warn-geometry: report geometry differences without failing. Text measurement depends on the
// browser and fonts installed, so re-measuring the same HTML on another machine moves frames a
// few pixels; missing nodes and changed text still fail.

import fs from 'node:fs';

const warnGeometry = process.argv.includes('--warn-geometry');
const [fa, fb] = process.argv.slice(2).filter((a) => !a.startsWith('--'));
if (!fa || !fb) {
  console.error('usage: compare.mjs a.mleaf.json b.mleaf.json');
  process.exit(2);
}
const load = (f) => new Map(JSON.parse(fs.readFileSync(f, 'utf8')).data.filter((n) => n.attributes?.['deck::frame'] || n.attributes?.['deck::slide']).map((n) => [n.path, n]));
const a = load(fa);
const b = load(fb);
const problems = [];
const warnings = [];
for (const p of a.keys()) if (!b.has(p)) problems.push(`${p}: only in ${fa}`);
for (const p of b.keys()) if (!a.has(p)) problems.push(`${p}: only in ${fb}`);

const textOf = (n) =>
  (n.attributes['deck::text']?.paragraphs || []).map((p) => ({
    text: p.runs.map((r) => (r.field ? `{${r.field}}` : r.text)).join(''),
    bullet: !!p.bullet,
    level: p.level || 0,
    runs: p.runs.filter((r) => r.text !== undefined).map((r) => [!!r.bold, !!r.italic, !!r.underline, r.sizePt, r.color]),
  }));

for (const [p, na] of a) {
  const nb = b.get(p);
  if (!nb) continue;
  const ga = na.attributes['deck::frame'];
  const gb = nb.attributes['deck::frame'];
  if (ga && gb) {
    for (const k of ['x', 'y', 'w', 'h']) {
      if (Math.abs(ga[k] - gb[k]) > 1) (warnGeometry ? warnings : problems).push(`${p}: ${k} ${ga[k]} → ${gb[k]}`);
    }
  }
  const ta = JSON.stringify(textOf(na));
  const tb = JSON.stringify(textOf(nb));
  if (ta !== tb) problems.push(`${p}: text differs\n    ${ta}\n    ${tb}`);
}
console.log(`compare: ${a.size} nodes vs ${b.size} nodes, ${problems.length} differences` + (warnGeometry ? `, ${warnings.length} geometry warnings` : ''));
problems.forEach((x) => console.log(`  ${x}`));
warnings.forEach((x) => console.log(`  warning: ${x}`));
process.exit(problems.length ? 1 : 0);
