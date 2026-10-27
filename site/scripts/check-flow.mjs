// Demo B on the site (/flow). Run on the box after cargo test -p milleleaf-web flow --release and the site build.
import { chromium } from '@playwright/test';
import { serve } from './serve.mjs';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';
const root = resolve('_shots');
const { server, origin: site } = await serve('dist/client');
const origin = `${site}/flow`;
const browser = await chromium.launch({ headless: true, args: ['--disable-gpu'] });
await mkdir(root, { recursive: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 1080 }, reducedMotion: 'reduce' });
page.setDefaultTimeout(180000);
const errors = [], requests = [];
page.on('pageerror', (error) => errors.push(error.message));
page.on('request', (request) => requests.push({ method: request.method(), url: request.url() }));
const asked = []; let replace = true;
page.on('dialog', (dialog) => { asked.push(dialog.message()); replace ? dialog.accept() : dialog.dismiss(); });
async function settled() {
  await page.waitForFunction(() => !document.getElementById('sample').disabled);
  assert.equal(await page.locator('#error').isVisible(), false, await page.locator('#error').textContent());
}
async function snapshot() { await settled(); return JSON.parse(await page.locator('#snapshot').textContent()); }
async function download(id, extension) {
  const ready = page.waitForEvent('download'); await page.locator(id).click();
  const file = await ready; assert(file.suggestedFilename().endsWith(extension));
  const stream = await file.createReadStream(); let bytes = 0; for await (const chunk of stream) bytes += chunk.length;
  assert(bytes > 500, 'The generated file must contain real content'); await settled();
}
try {
  await page.goto(origin); await settled();
  await page.screenshot({ path: `${root}/flow-start.png`, fullPage: true });
  const loaded = requests.length;
  await page.locator('#sample').click(); await settled();
  assert((await page.locator('#sheet option').count()) > 0);
  const workbookChartCount = await page.locator('#sheet option').evaluateAll((options) => options.reduce((count, option) => count + Number(option.dataset.chartCount), 0));
  assert.equal(await page.locator('#chart option').count(), workbookChartCount + 1, 'The chart selector includes every workbook chart plus the make-from-range option');
  const chartOptions = await page.locator('#chart option').allTextContents();
  const sheetCount = await page.locator('#sheet option').count();
  if (sheetCount > 1) {
    await page.locator('#sheet').selectOption({ index: sheetCount - 1 });
    assert.deepEqual(await page.locator('#chart option').allTextContents(), chartOptions, 'Changing the table sheet must retain all workbook chart options');
    await page.locator('#sheet').selectOption({ index: 0 });
  }
  await page.locator('#range').fill('A1:B4');
  await page.locator('#create').click(); let state = await snapshot();
  assert(state.report.table.rows.length > 0); assert(state.history.length >= 2);
  assert.deepEqual(state.report.table, state.deck.table);
  assert.equal(await page.locator('.live-preview').count(), 2);
  await download('#download-report', '.docx'); await download('#download-deck', '.pptx'); await download('#download-workbook', '.xlsx');
  await download('#download-project', '.json');
  await page.screenshot({ path: `${root}/flow-linked.png`, fullPage: true });
  await page.locator('#app').fill('GridCraft');
  await page.locator('#edited-file').setInputFiles('../_out/flow/finance-edited.xlsx'); state = await snapshot();
  assert(state.changes.length > 0, 'A changed workbook must report exact changed places');
  const changeLines = (await page.locator('#changes .change-item > p, #changes .cell-changes li').allTextContents()).join('\n');
  assert(!/[{}"]/.test(changeLines), 'Change review shows readable text, not raw JSON');
  assert(changeLines.includes('541,500 → 550,000'), 'A workbook edit lists the changed cell with its old and new value');
  assert(!changeLines.includes('541500'), 'Changed values use digit grouping and appear once');
  assert.equal(state.report.table.rows[1][1], 550000); assert.equal(state.deck.table.rows[1][1], 550000);
  assert.equal(state.deck.chart.series[0].values[1], 550000);
  assert.equal(state.history.at(-1).claimed_app, 'GridCraft');
  const importedLayerDetails = page.locator('#history > li').last().locator('.history-changes');
  assert.equal(await importedLayerDetails.getAttribute('open'), null, 'Layer change details start collapsed');
  const historyText = await importedLayerDetails.textContent();
  for (const place of ['report.docx', 'deck.pptx', 'Report table', 'Slide table']) assert(historyText.includes(place), `History must retain readable ${place} evidence`);
  assert.equal(await importedLayerDetails.locator(':scope > .change-item').count(), state.history.at(-1).changes.length + state.history.at(-1).conflicts.length, 'History lists every change and conflict without truncation');
  await page.locator('#project').scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${root}/flow-updated.png`, fullPage: true });
  assert.equal(await page.locator('#edited-file').inputValue(), '', 'The picker must reset after a successful import');
  const layersBeforeReselect = state.history.length;
  await page.locator('#edited-file').setInputFiles('../_out/flow/finance-edited.xlsx'); state = await snapshot();
  assert.equal(state.history.length, layersBeforeReselect + 1, 'Selecting the same file again must import another review layer');
  assert((await page.locator('#source-table').textContent()).includes('550,000'), 'Numeric preview values use digit grouping');
  await page.locator('#app').fill('Writer');
  await page.locator('#edited-file').setInputFiles('../_out/flow/report-conflict.docx'); state = await snapshot();
  assert(state.conflicts.length > 0, 'Typing over a linked value must be a conflict');
  assert.equal(state.report.table.rows[1][1], 550000);
  assert(await page.locator('#conflicts .conflict').count() > 0);
  const conflictText = await page.locator('#conflicts').textContent();
  for (const value of ['541,500', '999', '550,000']) assert(conflictText.includes(value), `The conflict must explain ${value} in context`);
  assert((await page.locator('#changes').textContent()).includes('No independent changes applied. Review the conflict below.'));
  await page.screenshot({ path: `${root}/flow-conflict.png`, fullPage: true });
  await page.locator('#edited-file').setInputFiles('../_out/flow/report-edited.docx'); state = await snapshot();
  assert(state.changes.length > 0, 'An independent report edit must return as a layer');
  await download('#download-report', '.docx'); await download('#download-deck', '.pptx');
  await download('#sign', '.json');
  assert((await page.locator('#signature').textContent()).includes('demo key'));
  // Opening another workbook over a project asks first; cancelling keeps the project.
  assert.equal(asked.length, 0, 'No question before a project exists');
  replace = false; await page.locator('#sample').click();
  await page.locator('#workbook-file').setInputFiles('../_out/flow/finance.xlsx');
  assert.equal(asked.length, 2, 'Both ways of opening a workbook ask first');
  assert(asked.every((message) => message.includes('discards the current project')));
  assert.equal(await page.locator('#project').isVisible(), true, 'Cancelling keeps the project');
  assert.equal(await page.locator('#download-report').isDisabled(), false);
  assert.equal(await page.locator('#workbook-file').inputValue(), '', 'The picker resets after cancelling');
  replace = true;
  // Opening another source starts a fresh project and must not offer stale exports.
  await page.locator('#sample').click(); await settled();
  assert.equal(asked.length, 3);
  assert.equal(await page.locator('#project').isVisible(), false);
  assert.equal(await page.locator('#download-report').isDisabled(), true);
  assert.equal(await page.locator('#download-deck').isDisabled(), true);
  assert.equal(await page.locator('.live-preview').count(), 0);
  const invalidWorkbook = { name: 'same-name.xlsx', mimeType: 'application/octet-stream', buffer: Buffer.from('not an archive') };
  await page.locator('#workbook-file').setInputFiles(invalidWorkbook);
  await page.waitForFunction(() => !document.getElementById('sample').disabled);
  assert.equal(await page.locator('#error').isVisible(), true);
  assert.equal(await page.locator('#workbook-file').inputValue(), '', 'The workbook picker resets after a failed read');
  await page.locator('#workbook-file').setInputFiles('../_out/flow/finance.xlsx'); await settled();
  assert.equal(await page.locator('#workbook-file').inputValue(), '');
  await page.locator('#workbook-file').setInputFiles('../_out/flow/finance.xlsx'); await settled();
  assert.equal(await page.locator('#workbook-file').inputValue(), '', 'The same workbook can be selected repeatedly');
  await page.locator('#range').fill('invalid range');
  await page.locator('#create').click();
  await page.waitForFunction(() => !document.getElementById('sample').disabled);
  assert.equal(await page.locator('#error').isVisible(), true);
  assert.equal(await page.locator('#project').isVisible(), false);
  await page.locator('#range').fill('A1:B4');
  await page.locator('#create').click(); await snapshot();
  const invalidReport = { name: 'same-name.docx', mimeType: 'application/octet-stream', buffer: Buffer.from('not an archive') };
  await page.locator('#edited-file').setInputFiles(invalidReport);
  await page.waitForFunction(() => !document.getElementById('sample').disabled);
  assert.equal(await page.locator('#error').isVisible(), true);
  assert.equal(await page.locator('#edited-file').inputValue(), '', 'The saved-file picker resets after a failed import');
  await page.locator('#edited-file').setInputFiles('../_out/flow/report-edited.docx'); await snapshot();
  await page.setViewportSize({ width: 390, height: 844 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Mobile must fit without horizontal overflow');
  await page.screenshot({ path: `${root}/flow-mobile.png`, fullPage: true });
  await page.setViewportSize({ width: 1400, height: 1080 });
  await page.locator('#workbook-file').setInputFiles('../examples/real/milleleaf-charts.gridcraft.xlsx'); await settled();
  const allChartCount = await page.locator('#sheet option').evaluateAll((options) => options.reduce((count, option) => count + Number(option.dataset.chartCount), 0));
  assert(allChartCount > 0);
  assert.equal(await page.locator('#chart option').count(), allChartCount + 1);
  const allChartOptions = await page.locator('#chart option').allTextContents();
  await page.locator('#sheet').selectOption({ label: 'Round trips' });
  assert.deepEqual(await page.locator('#chart option').allTextContents(), allChartOptions);
  const pieOption = await page.locator('#chart option').evaluateAll((options) => options.find((option) => option.textContent.includes('Formats ·') && option.textContent.includes('Pie'))?.value);
  assert(pieOption, 'The chart on another sheet must remain available');
  await page.locator('#chart').selectOption(pieOption);
  await page.locator('#range').fill('A2:A4'); await page.locator('#create').click(); state = await snapshot();
  assert.equal(state.report.table.rows[0][0], 'Mon');
  assert.deepEqual(state.deck.chart.series[0].values, [40, 35, 25]);
  await download('#download-report', '.docx'); await download('#download-deck', '.pptx');
  await page.screenshot({ path: `${root}/flow-cross-sheet.png`, fullPage: true });
  // A real workbook with hundreds of error cells (/flow once broke on lca-model.xlsx).
  await page.locator('#workbook-file').setInputFiles('../examples/real/lca-model.xlsx');
  await page.locator('#status[aria-busy="true"] .working-bar').waitFor({ timeout: 5000 });
  await settled();
  assert.match(await page.locator('#workbook-name').textContent(), /^lca-model\.xlsx · \d+(\.\d)? (KB|MB)$/, 'The chosen file shows its size');
  assert.equal(await page.locator('#status').getAttribute('aria-busy'), 'false', 'The busy cue ends when processing ends');
  assert.notEqual(await page.locator('#sheet option:checked').textContent(), 'Cover', 'Start on a sheet with numbers');
  assert((await page.locator('#source-warnings summary').textContent()).startsWith('Workbook notes ('), 'Notes are summarised');
  assert.equal(await page.locator('#source-warnings details').getAttribute('open'), null, 'Notes start folded');
  assert(await page.locator('#source-warnings p').count() < 50, 'Repeated cell notes are grouped');
  const suggested = await page.locator('#range').inputValue();
  assert.equal(suggested, 'B24:C40', 'The impact table with its header row (NaNMC), not a single row');
  await page.locator('#create').click(); state = await snapshot();
  assert.equal(await page.locator('.live-preview').count(), 2, `The suggested range ${suggested} makes a report and a deck`);
  await page.screenshot({ path: `${root}/flow-lca.png`, fullPage: true });
  console.log(`PASS lca-model.xlsx: starts on ${await page.locator('#sheet option:checked').textContent()} ${suggested}; notes grouped; report and deck made`);
  assert.equal(requests.length, loaded, 'File operations must not make network requests');
  assert(requests.every((request) => request.method === 'GET' && request.url.startsWith(site)));
  assert.deepEqual(errors, []);
  // A project exists, so leaving must ask first (it lives only in this tab).
  // A real beforeunload dialog needs a user gesture headless Chrome may not count; check the handler cancels instead.
  assert.equal(await page.evaluate(() => { const leave = new Event('beforeunload', { cancelable: true }); dispatchEvent(leave); return leave.defaultPrevented; }), true, 'Leaving with a project asks first');
  // The real page-host editors, used as a visitor would (no injected save events).
  {
    const editors = await browser.newContext({ viewport: { width: 1400, height: 1000 }, acceptDownloads: true });
    await editors.addInitScript(() => { window.confirm = () => true; });
    const flow = await editors.newPage();
    const flowErrors = [];
    flow.on('pageerror', (error) => flowErrors.push(error.message));
    flow.setDefaultTimeout(180_000);
    await flow.goto(origin);
    await flow.locator('#status').filter({ hasText: /^Ready\./ }).waitFor();
    await flow.locator('#sample').click();
    await flow.waitForFunction(() => !document.querySelector('#create').disabled);
    await flow.locator('#create').click();
    await flow.locator('#project:not([hidden])').waitFor();
    const history = () => flow.locator('#history > li').count();
    // The report opens in WordCraft straight after it is created, without a download first.
    await flow.locator('#open-wordcraft').click();
    await flow.locator('#wordcraft-editor canvas').first().waitFor();
    await flow.locator('#status').filter({ hasText: /open in WordCraft/ }).waitFor();
    assert.equal(await flow.locator('#error').isVisible(), false, 'WordCraft opens the current report without a prior download');
    // Text committed in one piece (as an input method does), then a key: the key follows it. The site's
    // global border-box rule must not reach the editor's hidden text field.
    await flow.waitForTimeout(4000);
    const word = await flow.locator('#wordcraft-editor canvas').first().boundingBox();
    await flow.mouse.click(word.x + word.width / 2, word.y + word.height * 0.6);
    await flow.waitForTimeout(500);
    const cdp = await editors.newCDPSession(flow);
    await cdp.send('Input.insertText', { text: 'ABC' });
    await flow.waitForTimeout(300);
    await flow.keyboard.press('y');
    await flow.waitForTimeout(500);
    assert.equal(await flow.evaluate(() => document.querySelector('#wordcraft-editor input[type=text]').value), 'ABCy', 'a key typed after committed text lands after it');
    // A real Save in GridCraft becomes a new layer of the project: history, report and deck stay.
    await flow.locator('#open-gridcraft').click();
    await flow.locator('#gridcraft-editor canvas').first().waitFor();
    await flow.waitForTimeout(6000);
    const grid = await flow.locator('#gridcraft-editor canvas').first().boundingBox();
    const before = await history();
    await flow.mouse.click(grid.x + 400, grid.y + 300);
    await flow.keyboard.type('77', { delay: 100 });
    await flow.keyboard.press('Enter');
    await flow.waitForTimeout(500);
    await flow.mouse.click(grid.x + 154, grid.y + 18); // GridCraft's own Save button
    await flow.waitForFunction((n) => document.querySelectorAll('#history > li').length > n, before, { timeout: 60_000 });
    assert.equal(await flow.locator('#project').isVisible(), true, 'a GridCraft save keeps the linked project');
    assert.equal(await flow.locator('#error').isVisible(), false, 'a GridCraft save brings no error');
    assert.deepEqual(flowErrors, [], 'no uncaught page errors while using the real editors');
    console.log(`PASS real editors: WordCraft opens the current report, typed text keeps its order, a GridCraft save adds layer ${await history()} and keeps the project`);
    await editors.close();
  }
  // Save-back: a stubbed File System Access API with an in-memory file. The page opens the
  // bundled workbook through the fake picker, records one layer, saves the workbook bytes into
  // the fake file, meets an outside edit as a conflict that writes nothing, and overwrites it
  // on request.
  {
    const savers = await browser.newContext({ viewport: { width: 1400, height: 1000 } });
    await savers.addInitScript((base64) => {
      const fake = {
        name: 'finance.xlsx',
        content: Uint8Array.from(atob(base64), (char) => char.charCodeAt(0)),
        lastModified: 1000,
        writes: 0,
        saveAsName: '',
      };
      const handle = {
        kind: 'file',
        name: fake.name,
        async getFile() { return new File([fake.content], fake.name, { lastModified: fake.lastModified }) },
        async queryPermission() { return 'granted' },
        async requestPermission() { return 'granted' },
        async createWritable() {
          const chunks = [];
          return {
            async write(chunk) { chunks.push(chunk instanceof Blob ? await chunk.arrayBuffer() : chunk) },
            async close() {
              // The page may write Blobs, ArrayBuffers or views; merge them into one file.
              const parts = []; let total = 0;
              for (const chunk of chunks) {
                const part = chunk instanceof ArrayBuffer ? new Uint8Array(chunk)
                  : ArrayBuffer.isView(chunk) ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)
                  : new Uint8Array(0);
                parts.push(part); total += part.length;
              }
              const merged = new Uint8Array(total); let at = 0;
              for (const part of parts) { merged.set(part, at); at += part.length }
              fake.content = merged; fake.writes += 1; fake.lastModified = 2000 + fake.writes;
            },
            async abort() {},
          };
        },
      };
      window.__fakeWorkbookFile = fake;
      window.showOpenFilePicker = async () => [handle];
      window.showSaveFilePicker = async (options) => { fake.saveAsName = options?.suggestedName ?? ''; return handle };
    }, (await readFile(resolve('../_out/flow/finance.xlsx'))).toString('base64'));
    const flow = await savers.newPage();
    const saveErrors = [];
    flow.on('pageerror', (error) => saveErrors.push(error.message));
    flow.setDefaultTimeout(180_000);
    await flow.goto(origin);
    await flow.locator('#workbook-pick').waitFor();
    await flow.locator('#status').filter({ hasText: /^Ready\./ }).waitFor();
    await flow.locator('#workbook-pick').click();
    await flow.locator('#status').filter({ hasText: 'Workbook ready' }).waitFor();
    assert((await flow.locator('#workbook-name').textContent()).includes('finance.xlsx'), 'the stubbed picker opened the workbook');
    // The status lives in the project section, which stays hidden until Create: wait for it attached, not visible.
    await flow.locator('#save-workbook-status').filter({ hasText: 'Saved to finance.xlsx' }).waitFor({ state: 'attached' });
    assert.equal(await flow.locator('#save-workbook-status').textContent(), 'Saved to finance.xlsx', 'the opened file counts as saved until an edit');
    await flow.locator('#create').click();
    await flow.locator('#save-workbook-status').filter({ hasText: 'Saved to finance.xlsx' }).waitFor();
    await flow.locator('#project:not([hidden])').waitFor();
    await flow.locator('#edited-file').setInputFiles('../_out/flow/finance-edited.xlsx');
    await flow.locator('#save-workbook-status').filter({ hasText: 'Unsaved changes' }).waitFor();
    await flow.locator('#save-workbook').click();
    await flow.locator('#save-workbook-status').filter({ hasText: 'Saved to finance.xlsx' }).waitFor();
    const saved = await flow.evaluate(() => { const fake = window.__fakeWorkbookFile; return { writes: fake.writes, head: String.fromCharCode(fake.content[0], fake.content[1]), length: fake.content.length } });
    assert.equal(saved.writes, 1, 'one write reached the file');
    assert.equal(saved.head, 'PK', 'the saved workbook bytes are a ZIP package');
    assert(saved.length > 500, 'the saved workbook holds real content');
    // The file changes on disk behind the page's back: the next save is a conflict that writes nothing.
    await flow.evaluate(() => { window.__fakeWorkbookFile.lastModified += 90000 });
    await flow.locator('#edited-file').setInputFiles('../_out/flow/finance-edited.xlsx');
    await flow.locator('#save-workbook-status').filter({ hasText: 'Unsaved changes' }).waitFor();
    await flow.locator('#save-workbook').click();
    await flow.locator('#save-workbook-status').filter({ hasText: 'changed on disk since you opened it.' }).waitFor();
    assert.equal(await flow.evaluate(() => window.__fakeWorkbookFile.writes), 1, 'a conflict writes nothing');
    assert(await flow.locator('#save-overwrite').isVisible() && await flow.locator('#save-as').isVisible(), 'the conflict offers Overwrite anyway and Save as…');
    await flow.locator('#save-overwrite').click();
    await flow.locator('#save-workbook-status').filter({ hasText: 'Saved to finance.xlsx' }).waitFor();
    const overwritten = await flow.evaluate(() => { const fake = window.__fakeWorkbookFile; return { writes: fake.writes, head: String.fromCharCode(fake.content[0], fake.content[1]) } });
    assert.equal(overwritten.writes, 2, 'Overwrite anyway wrote the file');
    assert.equal(overwritten.head, 'PK');
    assert.equal(await flow.locator('#error').isVisible(), false, 'the save-back flow raises no error');
    assert.deepEqual(saveErrors, [], 'no uncaught page errors in the save-back flow');
    console.log(`PASS save-back: stubbed pickers opened the workbook, one recorded layer showed unsaved, Save wrote ${saved.length} PK bytes into the fake file, an outside edit became a conflict that wrote nothing, and Overwrite anyway wrote it`);
    await savers.close();
  }
  console.log('PASS browser flow: bundled workbook, sheet/range/chart, real Office downloads, both linked outputs updated, readable cell changes, linked-value conflict, independent report edit, claimed app history, demo proof, local-only operations, mobile layout, replace and leave warnings');
} finally { await browser.close(); await new Promise((done) => server.close(done)); }
