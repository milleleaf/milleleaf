// Demo A on the site (/demo). Run on the box after the site build (site/README.md).
import { chromium } from '@playwright/test';
import { serve } from './serve.mjs';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, basename } from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';

const root = resolve('_shots');
const serverRequests = [];
let serverPhase = 'load';
const { server, origin: site } = await serve('dist/client', 0, (request) => serverRequests.push({ phase: serverPhase, ...request }));
const origin = `${site}/demo`;
const browser = await chromium.launch({ headless: true, args: ['--disable-gpu'] });
await mkdir(root, { recursive: true });
const cases = [
  ['resave', '../examples/real/paper.docx', '../examples/real/fixtures/paper.msoffice-resaved.docx', 0],
  ['blank-line', '../examples/real/paper.docx', '../examples/real/fixtures/paper.msoffice-blank-line.docx', 1],
  ['workbook', '../examples/real/lca-model.xlsx', '../examples/real/fixtures/lca-model.msoffice-margins.xlsx', 7],
  ['deck', '../examples/real/milleleaf-deck.impress.pptx', '../examples/real/fixtures/milleleaf-deck.msoffice-edited.pptx', 1],
  ['edited', '../_out/demo-a/finance.xlsx', '../examples/hello-table/fixtures/finance.openpyxl-edited.xlsx', 2],
];
async function ready(page) {
  await page.waitForFunction(() => document.getElementById('status').textContent.startsWith('Ready.') || !document.getElementById('error').hidden);
  assert.equal(await page.locator('#error').isVisible(), false, await page.locator('#error').textContent());
  await page.waitForLoadState('load');
  await page.locator('img').evaluateAll(async (images) => Promise.all(images.map((image) => image.decode())));
  await page.waitForLoadState('networkidle');
}
async function visible(page, id) {
  // A new comparison keeps the previous result visible while reading metadata.
  // Settle the operation first so old results cannot conceal a later import error.
  await page.waitForFunction(() => !document.body.classList.contains('busy') && !document.getElementById('files').disabled);
  assert.equal(await page.locator('#error').isVisible(), false, await page.locator('#error').textContent());
  assert.equal(await page.locator(`#${id}`).evaluate((element) => element.hidden), false,
    `${id} remained hidden after operation: ${await page.locator('#status').textContent()}`);
}
// The box already provides Python. Alter only metadata in memory for the fallback probe.
function withoutModified(path) {
  return execFileSync('python3', ['-c', `import io,re,sys,zipfile
out=io.BytesIO()
with zipfile.ZipFile(sys.argv[1]) as src, zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as dst:
 for item in src.infolist():
  data=src.read(item.filename)
  if item.filename=='docProps/core.xml': data=re.sub(rb'<dcterms:modified[^>]*>.*?</dcterms:modified>',b'',data,flags=re.S)
  dst.writestr(item,data)
sys.stdout.buffer.write(out.getvalue())`, path], { maxBuffer: 64 * 1024 * 1024 });
}
function withTextEdit(path) {
  return execFileSync('python3', ['-c', `import io,sys,zipfile
out=io.BytesIO()
changed=False
with zipfile.ZipFile(sys.argv[1]) as src, zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as dst:
 for item in src.infolist():
  data=src.read(item.filename)
  if item.filename=='word/document.xml':
   previous=data
   data=data.replace(b'Throughout this document',b'Throughout the document',1)
   changed=data!=previous
  dst.writestr(item,data)
assert changed, 'Expected paragraph text was absent from the fixture'
sys.stdout.buffer.write(out.getvalue())`, path], { maxBuffer: 64 * 1024 * 1024 });
}
try {
  const titled = await browser.newPage();
  await titled.goto(origin);
  await ready(titled);
  assert.equal(await titled.title(), 'Compare two versions: demo | Milleleaf');
  // A file over 64 MiB is refused before it is read.
  const big = await titled.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([new Uint8Array(64 * 1024 * 1024 + 1)], 'big.docx'));
    return transfer;
  });
  await titled.locator('#files-drop').dispatchEvent('drop', { dataTransfer: big });
  await big.dispose();
  await titled.waitForFunction(() => !document.getElementById('error').hidden);
  assert.match(await titled.locator('#error').textContent(), /big\.docx is 64\.0 MB\. This demo accepts files up to 64 MiB\./);
  await titled.close();
  console.log('PASS page: site layout, title, engine ready');
  for (const [name, original, edited, expectedCount] of cases) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    page.setDefaultTimeout(180_000);
    const errors = [];
    const requests = [];
    page.on('pageerror', (e) => errors.push(e.message));
    let requestPhase = 'load';
    page.context().on('request', (req) => requests.push({ phase: requestPhase, method: req.method(), url: req.url() }));
    await page.goto(origin);
    await ready(page);
    requestPhase = 'operations';
    serverPhase = `${name}:operations`;
    if (process.env.CHECK_OFFLINE === '1') await page.context().setOffline(true);
    assert.equal(await page.locator('#compare').count(), 0, 'Comparison starts automatically');
    assert.equal(await page.locator('#author').isVisible(), false, 'Author is asked only when signing');
    if (name === 'blank-line') {
      const files = [
        { name: basename(edited), bytes: [...await readFile(edited)], lastModified: 1 },
        { name: basename(original), bytes: [...await readFile(original)], lastModified: Date.now() },
      ];
      const transfer = await page.evaluateHandle((files) => {
        const transfer = new DataTransfer();
        for (const file of files) transfer.items.add(new File([Uint8Array.from(file.bytes)], file.name, { lastModified: file.lastModified }));
        return transfer;
      }, files);
      await page.locator('#files-drop').dispatchEvent('drop', { dataTransfer: transfer });
      await transfer.dispose();
    } else await page.locator('#files').setInputFiles([original, edited]);
    await visible(page, 'results');
    await page.waitForFunction(() => !document.getElementById('files').disabled);
    // The generated example can be newer than its saved fixture; respect detection,
    // then use the user-facing override to compare this fixture in its intended direction.
    if (name === 'edited' && await page.locator('#original-name').textContent() === basename(edited)) {
      await page.locator('#swap').click();
      await visible(page, 'results');
      await page.waitForFunction(() => !document.getElementById('swap').disabled);
    }
    assert.equal(await page.locator('#original-name').textContent(), basename(original), `${name}: timestamp order detects original`);
    assert.equal(await page.locator('#edited-name').textContent(), basename(edited));
    const report = JSON.parse(await page.locator('#complete-report').textContent());
    assert.equal(report.format, original.split('.').at(-1));
    assert.equal(report.changes.length, expectedCount, `${name}: expected content changes`);
    if (name === 'edited') {
      for (const [cell, before, after] of [['B3', '541500', '550000'], ['B5', '1250500', '1259000']]) {
        assert(report.changes.some((change) => {
          const readable = `${change.locator} ${change.description}`.replaceAll(',', '');
          return readable.includes('Q3') && readable.includes(cell) && readable.includes(before) && readable.includes(after);
        }), `Missing readable edit at Sheet Q3 cell ${cell}: ${before} → ${after}`);
      }
    }
    assert.equal(await page.locator('#changes .report-item').count(), report.changes.length);
    assert(report.changes.every((change) => change.locator && change.description && change.context), 'Every change needs a readable locator and context');
    assert.equal(await page.locator('#changes .context-panel').count(), report.changes.length * 2);
    assert.equal(await page.locator('#changes details[open]').count(), 0, 'Technical detail starts collapsed');
    assert.equal(await page.locator('#changes summary').filter({ hasText: /^Developer detail$/ }).count(), report.changes.length);
    const answer = await page.locator('#comparison-summary').textContent();
    assert.equal(answer, report.summary);
    assert.equal(await page.locator('#results > details[open]').count(), 0, 'Inventory, limitations, and signing start folded');
    assert.equal(await page.locator('#losses-title').textContent(), 'Changes or removals in the edited file');
    assert.equal(await page.locator('#adapter-losses-title').textContent(), 'What Milleleaf cannot carry yet');
    assert((await page.locator('.signature').textContent()).includes('demo key'));
    assert(report.adapter_losses.original && report.adapter_losses.edited);
    if (name === 'resave') {
      assert.match(answer, /^No content changes\./);
      await page.screenshot({ path: `${root}/demo-comparison.png` });
    }
    if (name === 'blank-line') {
      assert.match(answer, /empty line/i);
      assert.match(answer, /Throughout this document/);
      assert.match(report.changes[0].locator, /Throughout this document/);
      assert.match(await page.locator('#order-note').textContent(), /document modified time/);
      for (const selector of ['#comparison-summary', '#changes .report-item:first-child .context-sides']) {
        const bounds = await page.locator(selector).boundingBox();
        assert(bounds && bounds.y >= 0 && bounds.y + bounds.height <= 800, `${selector}: answer and complete context fit the first screen`);
      }
      await page.screenshot({ path: `${root}/demo-blank-line.png` });
      await page.setViewportSize({ width: 390, height: 844 });
      await page.locator('#comparison-summary').scrollIntoViewIfNeeded();
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Mobile layout must not overflow');
      const panels = await page.locator('#changes .context-panel').all();
      const before = await panels[0].boundingBox();
      const after = await panels[1].boundingBox();
      assert(after.y >= before.y + before.height, 'Context stacks on a phone');
      await page.screenshot({ path: `${root}/demo-mobile.png` });
      await page.setViewportSize({ width: 1280, height: 800 });
      await page.locator('#swap').click();
      await visible(page, 'results');
      await page.waitForFunction(() => !document.getElementById('swap').disabled);
      assert.equal(await page.locator('#original-name').textContent(), basename(edited));
      console.log('PASS blank-line stage: swapped original and edited');
      await page.locator('#swap').click();
      await visible(page, 'results');
      await page.waitForFunction(() => !document.getElementById('swap').disabled);
      assert.equal(await page.locator('#comparison-summary').textContent(), answer, 'Swap back restores the answer');
      console.log('PASS blank-line stage: swap back restored answer');
      // Changing archive bytes changes an unmarked original's adopted identity.
      // Reuse the same unmarked bytes for this ordering-only probe so a marked
      // edited fixture is never falsely paired with a different adopted document.
      const fallbackBytes = [...withoutModified(original)];
      const fallbackFiles = [
        { name: 'fallback-edited.docx', bytes: fallbackBytes, lastModified: 2000 },
        { name: 'fallback-original.docx', bytes: fallbackBytes, lastModified: 1000 },
      ];
      const fallback = await page.evaluateHandle((files) => {
        const transfer = new DataTransfer();
        for (const file of files) transfer.items.add(new File([Uint8Array.from(file.bytes)], file.name, { lastModified: file.lastModified }));
        return transfer;
      }, fallbackFiles);
      await page.locator('#files-drop').dispatchEvent('drop', { dataTransfer: fallback });
      await fallback.dispose();
      console.log('CHECK blank-line stage: dropped pair without core modified times');
      await visible(page, 'results');
      await page.waitForFunction(() => !document.getElementById('files').disabled);
      assert.equal(await page.locator('#original-name').textContent(), 'fallback-original.docx');
      assert.equal(await page.locator('#edited-name').textContent(), 'fallback-edited.docx');
      assert.match(await page.locator('#order-note').textContent(), /file modified time/);
      assert.equal(await page.locator('#comparison-summary').textContent(), 'No content changes. The file was re-saved.', 'Missing core times use File.lastModified');
      console.log('PASS blank-line stage: fallback timestamps and context ready');
      // Keep the fixture identity and its empty line, adding one substantive edit
      // to check that spacing context stays visible alongside other changes.
      const mixedFiles = [
        { name: basename(original), bytes: [...await readFile(original)] },
        { name: 'mixed-edit.docx', bytes: [...withTextEdit(edited)] },
      ];
      const mixed = await page.evaluateHandle((files) => {
        const transfer = new DataTransfer();
        for (const file of files) transfer.items.add(new File([Uint8Array.from(file.bytes)], file.name));
        return transfer;
      }, mixedFiles);
      await page.locator('#files-drop').dispatchEvent('drop', { dataTransfer: mixed });
      await mixed.dispose();
      await visible(page, 'results');
      const mixedReport = JSON.parse(await page.locator('#complete-report').textContent());
      assert.equal(mixedReport.changes.length, 2, 'Mixed fixture has text and empty-line edits');
      assert.equal(mixedReport.changes.filter((change) => change.spacing_only).length, 1);
      const spacingHeading = page.getByRole('heading', { name: 'Empty lines and spacing (1)', exact: true });
      assert.equal(await spacingHeading.isVisible(), true, 'Mixed spacing group is visible');
      const spacingPanels = spacingHeading.locator('..').locator('.context-panel');
      assert.equal(await spacingPanels.count(), 2);
      for (const panel of await spacingPanels.all()) assert.equal(await panel.isVisible(), true, 'Spacing before/after context is visible without opening a disclosure');
      assert.equal(await page.locator('#changes details[open]').count(), 0, 'Mixed Developer details remain closed');
      console.log('PASS blank-line stage: mixed spacing context stays visible');
      await page.locator('#files').setInputFiles([original, edited]);
      await visible(page, 'results');
      assert.equal(await page.locator('#comparison-summary').textContent(), answer, 'Restore the real blank-line pair before signing');
      console.log('PASS blank-line stage: real fixture restored before signing');
    }
    if (name === 'workbook' || name === 'deck') await page.screenshot({ path: `${root}/demo-${name}.png` });
    if (name === 'workbook') {
      const pictureIndex = report.changes.findLastIndex((change) => change.description.startsWith('Pictures'));
      assert(pictureIndex >= 0, 'The margins fixture includes the resized legend picture');
      const change = report.changes[pictureIndex];
      const thumbnails = ['before', 'after'].flatMap((side) => change.context[side]?.media || []).filter((media) => media.thumbnail).length;
      const picture = page.locator('#changes .report-item').nth(pictureIndex);
      assert.equal(await picture.locator('img').count(), thumbnails, 'Every available picture thumbnail is rendered');
      await picture.screenshot({ path: `${root}/demo-picture.png` });
    }
    if (name === 'edited') {
      assert(await page.locator('#changes .context-grid table').count() >= 2, 'Cell edits render neighbouring grids');
      await page.locator('#comparison-summary').scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${root}/demo-cells.png` });
    }
    await page.locator('#signing > summary').click();
    await page.locator('#author').fill('Demo reviewer');
    await page.locator('#sign').click();
    await visible(page, 'signature-result');
    const proof = JSON.parse(await page.locator('#proof').textContent());
    const verification = JSON.parse(await page.locator('#verify-report').textContent());
    assert.equal(verification.valid, true, JSON.stringify(verification));
    assert.equal(proof.signatures.length, 2);
    assert.equal(verification.author, 'Demo reviewer');
    assert((await page.locator('#signed-author').textContent()).includes(proof.layer.header.author));
    assert((await page.locator('#signed-at').textContent()).includes(proof.layer.header.timestamp));
    assert.deepEqual(requests.filter((request) => request.phase === 'operations'), [], 'Drop, compare, sign and verify must make zero requests, including workers');
    assert(requests.every((r) => r.method === 'GET' &&
      r.url.startsWith(`${site}/`)),
    'Startup resources must come only from this site');
    assert.deepEqual(errors, []);
    assert.equal(await page.locator('#comparison-summary').textContent(), answer, 'Signing preserves the answer about the file');
    if (name === 'edited') {
      await page.locator('#signing').scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${root}/demo-signature.png` });
      requestPhase = 'reload';
      serverPhase = 'reload';
      await page.context().setOffline(false);
      await page.reload();
      await ready(page);
      requestPhase = 'operations';
      serverPhase = `${name}:operations`;
      if (process.env.CHECK_OFFLINE === '1') await page.context().setOffline(true);
      await page.locator('details.verify-file > summary').click();
      await page.locator('#proof-file').setInputFiles({ name: 'proof.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(proof)) });
      await visible(page, 'saved-verify-details');
      assert.equal(JSON.parse(await page.locator('#saved-verify-report').textContent()).valid, true);
      const author = proof.layer.header.author;
      proof.layer.header.author = 'Tampered';
      await page.locator('#proof-file').setInputFiles({ name: 'tampered.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(proof)) });
      await page.waitForFunction(() => document.getElementById('saved-verification').textContent.includes('failed'));
      proof.layer.header.author = author;
      proof.signatures = proof.signatures.filter((s) => s.alg !== 'mldsa65');
      await page.locator('#proof-file').setInputFiles({ name: 'missing-pq.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(proof)) });
      await visible(page, 'saved-verify-details');
      assert.equal(JSON.parse(await page.locator('#saved-verify-report').textContent()).valid, false);
    }
    // Include deferred callbacks and any unload attempts in the monitored phase.
    await page.waitForTimeout(600);
    await page.close();
    assert.deepEqual(requests.filter((request) => request.phase === 'operations'), [], 'All file operations, including fresh-session verification, must make zero requests');
    assert.deepEqual(serverRequests.filter((request) => request.phase.endsWith(':operations')), [], 'The static server received no requests during file operations');
    await writeFile(`${root}/demo-requests-${process.env.CHECK_OFFLINE === '1' ? 'offline-' : ''}${name}.json`, JSON.stringify({ browser: requests, server: serverRequests }, null, 2));
    serverPhase = 'load';
    console.log(`PASS ${name}: ${answer} (${report.changes.length} changes; hybrid signature valid)`);
  }
  console.log('PASS browser checks: automatic comparison, reversed drop order, core timestamps and file-time fallback, Swap, first-screen context, folded JSON, mobile stacking, local-only requests, portable proof, tamper rejection, no page errors');
} finally {
  await browser.close();
  await new Promise((done) => server.close(done));
}
