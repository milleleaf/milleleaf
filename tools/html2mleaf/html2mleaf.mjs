#!/usr/bin/env node
// html2mleaf — measure an HTML deck written to the Milleleaf HTML deck profile and write it as an Milleleaf
// proposal layer (plus assets and a loss report).
//
//   node html2mleaf.mjs deck.html --out deck.proposal.mleaf.json [--onto base.mleaf.json]
//        [--attach <parent-path>[:<child-name>]] [--assets DIR] [--author NAME] [--id ID]
//        [--allow-js] [--raster-fallback] [--strict]

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { extractDeck } = require('./extract.cjs');

function loadPlaywright() {
  try {
    return require('playwright');
  } catch {
    // Fall back to a global install (npm root -g).
    const { execSync } = require('node:child_process');
    const root = execSync('npm root -g').toString().trim();
    return require(path.join(root, 'playwright'));
  }
}

function parseArgs(argv) {
  const a = { flags: new Set(), opts: {}, pos: [] };
  const valued = new Set(['--out', '--onto', '--attach', '--assets', '--author', '--id']);
  for (let i = 0; i < argv.length; i++) {
    const x = argv[i];
    if (valued.has(x)) a.opts[x.slice(2)] = argv[++i];
    else if (x.startsWith('--')) a.flags.add(x.slice(2));
    else a.pos.push(x);
  }
  return a;
}

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');
/** Deterministic UUID-shaped id, so re-importing the same deck gives the same paths. */
function stableId(seed) {
  const h = sha256(seed);
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-5${h.slice(13, 16)}-${((parseInt(h[16], 16) & 3) | 8).toString(16)}${h.slice(17, 20)}-${h.slice(20, 32)}`;
}
const posix = (p) => p.split(path.sep).join('/');

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const input = args.pos[0];
  if (!input || !args.opts.out || args.flags.has('help')) {
    console.error('usage: html2mleaf.mjs deck.html --out deck.proposal.mleaf.json [--onto base.mleaf.json] [--attach parent[:name]] [--assets DIR] [--author NAME] [--id ID] [--allow-js] [--raster-fallback] [--strict]');
    process.exit(2);
  }
  const htmlPath = path.resolve(input);
  const outPath = path.resolve(args.opts.out);
  const outDir = path.dirname(outPath);
  const assetsDir = path.resolve(args.opts.assets || path.join(outDir, 'assets'));
  const htmlBytes = fs.readFileSync(htmlPath);
  const base = path.basename(htmlPath);

  const { chromium } = loadPlaywright();
  const browser = await chromium.launch();
  const context = await browser.newContext({ javaScriptEnabled: args.flags.has('allow-js'), viewport: { width: 1280, height: 720 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  await page.goto(pathToFileURL(htmlPath).href, { waitUntil: 'load' });

  const animations = args.flags.has('allow-js') ? await page.evaluate(() => document.getAnimations().length) : 0;
  const scripts = await page.evaluate(() => document.querySelectorAll('script').length);
  // Lay every slide out at its own size, one under another, with motion switched off.
  // (page.addStyleTag waits on a load event that never fires with JavaScript off.)
  await page.evaluate((css) => { const s = document.createElement('style'); s.textContent = css; document.head.appendChild(s); }, `
    *, *::before, *::after { animation: none !important; transition: none !important; }
    [data-ox-slide] { display: block !important; visibility: visible !important; opacity: 1 !important;
      position: relative !important; left: auto !important; top: auto !important; transform: none !important; }`);
  await page.evaluate(() => document.fonts.ready);
  const deck = await page.evaluate(extractDeck);

  const deckIssues = [...deck.issues];
  if (scripts && !args.flags.has('allow-js')) deckIssues.push({ code: 'scripts-off', severity: 'warning', message: `${scripts} <script> element(s) not run; pass --allow-js to measure what they draw` });
  if (animations) deckIssues.push({ code: 'animation', severity: 'info', message: `${animations} animation(s) dropped` });

  // ---- assets -------------------------------------------------------------------------
  fs.mkdirSync(assetsDir, { recursive: true });
  const writeAsset = (bytes, ext) => {
    const file = path.join(assetsDir, `${sha256(bytes).slice(0, 16)}.${ext}`);
    fs.writeFileSync(file, bytes);
    return posix(path.relative(outDir, file));
  };
  const extOf = (mime, fallback) => ({ 'image/png': 'png', 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp', 'image/svg+xml': 'svg' })[mime] || fallback;
  async function screenshot(tmp) {
    return writeAsset(await page.locator(`[data-ox-tmp="${tmp}"]`).screenshot({ omitBackground: true }), 'png');
  }
  async function resolveAsset(frame) {
    const a = frame.asset;
    if (a.svg) return writeAsset(Buffer.from(a.svg), 'svg');
    if (a.screenshot) {
      if (frame.kind === 'unsupported' && !args.flags.has('raster-fallback')) return null;
      return screenshot(a.tmp);
    }
    const src = a.src || '';
    if (src.startsWith('data:')) {
      const m = src.match(/^data:([^;,]+)(;base64)?,(.*)$/s);
      if (m) return writeAsset(m[2] ? Buffer.from(m[3], 'base64') : Buffer.from(decodeURIComponent(m[3])), extOf(m[1], 'bin'));
    }
    if (src.startsWith('file:')) {
      const p = fileURLToPath(src);
      if (fs.existsSync(p)) return writeAsset(fs.readFileSync(p), path.extname(p).slice(1) || 'bin');
    }
    frame.loss.push({ code: 'remote-image', severity: 'warning', message: `image ${src.slice(0, 80)} is not local` + (args.flags.has('raster-fallback') ? '; captured as a screenshot' : '; dropped (use --raster-fallback)') });
    return args.flags.has('raster-fallback') ? screenshot(a.tmp) : null;
  }

  // ---- layer --------------------------------------------------------------------------
  const deckPath = deck.deckPath || stableId(`deck|${base}`);
  const nodes = [];
  const report = [];
  const note = (p, name, list) => list.forEach((l) => report.push({ path: p, name, ...l }));
  const slideChildren = {};
  const slideOrder = [];
  const usedSlideNames = new Set();
  const unique = (used, want) => { let n = want, i = 2; while (used.has(n)) n = `${want}-${i++}`; used.add(n); return n; };

  for (const s of deck.slides) {
    const sName = unique(usedSlideNames, s.name || `slide-${s.index + 1}`);
    const sPath = s.path || stableId(`${deckPath}|slide|${sName}`);
    slideChildren[sName] = sPath;
    slideOrder.push(sName);
    const frameChildren = {};
    const frameOrder = [];
    const usedFrameNames = new Set();
    const counters = {};
    const sLoss = [...s.loss];
    let z = 0;
    for (const fr of s.frames) {
      if (fr.kind === 'container-loss') { sLoss.push(...fr.loss.map((l) => ({ ...l, message: `container: ${l.message}` }))); continue; }
      counters[fr.kind] = (counters[fr.kind] || 0) + 1;
      const fName = unique(usedFrameNames, fr.name || `${fr.kind}-${counters[fr.kind]}`);
      const fPath = fr.path || stableId(`${sPath}|frame|${fName}`);
      const node = { path: fPath, attributes: { 'deck::frame': { ...fr.geom, z: z++ } } };
      const attrs = node.attributes;

      if (fr.kind === 'text') {
        const children = {};
        const paragraphs = fr.paragraphs.map((p) => ({
          ...p,
          runs: p.runs.map((r) => {
            if (r.fieldIndex === undefined) return r;
            const f = fr.fields[r.fieldIndex];
            const name = f.name || `field-${r.fieldIndex + 1}`;
            const childPath = f.path || stableId(`${fPath}|field|${name}`);
            children[name] = childPath;
            nodes.push({ path: childPath, inherits: { source: f.link }, attributes: { 'mleaf::link': { source: f.link, mode: f.mode } } });
            fr.loss.push({ code: 'linked-field', severity: 'info', message: `field "${name}" shows ${f.link}; the HTML's "${f.text}" was not imported` });
            return { field: name, ...r.style };
          }),
        }));
        attrs['deck::text'] = { role: fr.role, paragraphs };
        if (Object.keys(children).length) node.children = children;
      } else if (fr.kind === 'table') {
        attrs['deck::table'] = { style: fr.table.style };
        if (fr.link && fr.linkMode !== 'pinned' && fr.linkMode !== 'detached') {
          node.inherits = { source: fr.link };
          attrs['mleaf::link'] = { source: fr.link, mode: 'live' };
          fr.loss.push({ code: 'linked-table', severity: 'info', message: `table content comes from ${fr.link}; the HTML's copy (${fr.table.rows.length} rows) was not imported` });
        } else {
          attrs['deck::table'].snapshot = fr.table.rows;
          if (fr.link) attrs['mleaf::link'] = { source: fr.link, mode: fr.linkMode };
        }
      } else if (fr.kind === 'picture' || fr.kind === 'vector' || fr.kind === 'unsupported') {
        const blob = await resolveAsset(fr);
        if (!blob) { note(fPath, fName, fr.loss); report.push({ path: fPath, name: fName, code: 'dropped', severity: 'warning', message: `${fr.kind} frame dropped` }); z--; continue; }
        attrs[fr.kind === 'vector' ? 'deck::vector' : 'deck::picture'] = { blob, ...(fr.alt ? { alt: fr.alt } : {}) };
      } else if (fr.kind === 'chart') {
        node.inherits = { source: fr.link };
        attrs['deck::chart'] = fr.chart;
        attrs['mleaf::link'] = { source: fr.link, mode: 'live' };
        fr.loss.push({ code: 'linked-chart', severity: 'info', message: `chart is drawn from ${fr.link}; the HTML's picture was not imported` });
      } else if (fr.kind === 'shape') {
        attrs['deck::shape'] = fr.shape;
      }
      if (fr.loss.length) attrs['mleaf::loss'] = fr.loss;
      note(fPath, fName, fr.loss);
      frameChildren[fName] = fPath;
      frameOrder.push(fName);
      nodes.push(node);
    }
    const slideNode = { path: sPath, children: frameChildren, order: frameOrder, attributes: { 'deck::slide': { name: sName } } };
    if (s.background) slideNode.attributes['deck::slide'].background = s.background;
    if (s.notes) slideNode.attributes['deck::slide'].notes = s.notes;
    if (sLoss.length) slideNode.attributes['mleaf::loss'] = sLoss;
    note(sPath, sName, sLoss);
    nodes.push(slideNode);
  }

  const deckNode = {
    path: deckPath,
    children: slideChildren,
    order: slideOrder,
    attributes: {
      'deck::deck': { width: deck.W, height: deck.H, unit: 'px', ...(deck.title ? { title: deck.title } : {}) },
      'mleaf::source': { format: 'html-deck/0', file: base, sha256: sha256(htmlBytes) },
    },
  };
  if (deckIssues.length) deckNode.attributes['mleaf::loss'] = deckIssues;
  note(deckPath, 'deck', deckIssues);

  const data = [deckNode, ...nodes];
  if (args.opts.attach) {
    const [parent, childName] = args.opts.attach.split(':');
    data.unshift({ path: parent, children: { [childName || path.basename(base, path.extname(base))]: deckPath } });
  }
  const imports = [];
  if (args.opts.onto) {
    const onto = path.resolve(args.opts.onto);
    imports.push({ uri: posix(path.relative(outDir, onto)), integrity: `sha256-${sha256(fs.readFileSync(onto))}` });
  }
  const layer = {
    header: {
      id: args.opts.id || `proposal/${path.basename(base, path.extname(base))}`,
      mleafVersion: 'mleaf_alpha',
      dataVersion: '1.0.0',
      author: args.opts.author || 'html2mleaf',
      timestamp: new Date().toISOString().slice(0, 10),
      kind: 'proposal',
    },
    imports,
    schemas: {},
    data,
  };
  fs.writeFileSync(outPath, JSON.stringify(layer, null, 2) + '\n');

  const counts = { info: 0, warning: 0, error: 0 };
  report.forEach((r) => counts[r.severity]++);
  const reportPath = outPath.replace(/(\.mleaf)?\.json$/, '') + '.loss.json';
  fs.writeFileSync(reportPath, JSON.stringify({ source: base, layer: posix(path.relative(outDir, outPath)), counts, fonts: deck.fonts, items: report }, null, 2) + '\n');

  console.error(`html2mleaf: ${deck.slides.length} slides, ${nodes.length - deck.slides.length} frames → ${path.relative(process.cwd(), outPath)}`);
  console.error(`html2mleaf: loss report: ${counts.error} errors, ${counts.warning} warnings, ${counts.info} info → ${path.relative(process.cwd(), reportPath)}`);
  for (const r of report.filter((r) => r.severity !== 'info')) console.error(`  ${r.severity}: ${r.name}: ${r.message}`);
  await browser.close();
  if (args.flags.has('strict') && counts.error) process.exit(1);
}

main().catch((e) => {
  console.error(`html2mleaf: ${e.stack || e}`);
  process.exit(2);
});
