// Privacy checks for the site's demo and office pages: files never leave the browser.
// Run on the box after the site build.
// /demo: offline drop, compare, sign and verify, zero network requests after load.
// /flow and /office: the page-host editors load from /apps/* on this site and are allowed to
// connect only to this origin; the check fails on any request to another origin and counts
// every request so an editor save must land in the desk without touching the network beyond it.
import { chromium } from '@playwright/test';
import { serve } from './serve.mjs';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, basename } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve('_shots');
const received = [];
const { server, origin: site } = await serve('dist/client', 0, (request) => received.push(request));
const browser = await chromium.launch({ headless: true, args: ['--disable-gpu'] });
await mkdir(root, { recursive: true });

async function ready(page, diagnostics) {
  try {
    await page.waitForFunction(() => document.getElementById('status').textContent.startsWith('Ready.') || !document.getElementById('error').hidden);
  } catch (error) {
    throw new Error(`${error.message}\nStartup diagnostics: ${JSON.stringify(diagnostics, null, 2)}`);
  }
  assert.equal(await page.locator('#error').isVisible(), false, `${await page.locator('#error').textContent()}\nStartup diagnostics: ${JSON.stringify(diagnostics, null, 2)}`);
  await page.waitForLoadState('load');
  await page.locator('img').evaluateAll(async (images) => Promise.all(images.map((image) => image.decode())));
  await page.waitForLoadState('networkidle');
}
async function settled(page, id) {
  await page.waitForFunction(() => !document.body.classList.contains('busy') && !document.getElementById('files').disabled);
  assert.equal(await page.locator('#error').isVisible(), false, await page.locator('#error').textContent());
  assert.equal(await page.locator(`#${id}`).evaluate((element) => element.hidden), false);
}
// Intentionally blocked attempts have their own phase, separate from file operations.
async function blockedFetch(target, url) {
  return target.evaluate(async (url) => {
    const violations = [];
    const record = (event) => violations.push({ directive: event.effectiveDirective, blocked: event.blockedURI });
    globalThis.addEventListener('securitypolicyviolation', record);
    let rejected = false;
    try { await fetch(url); } catch (error) { rejected = error instanceof TypeError; }
    await new Promise((done) => setTimeout(done, 100));
    globalThis.removeEventListener('securitypolicyviolation', record);
    return { rejected, violations };
  }, url);
}
// Every request the /flow and /office pages or their editors make, as the browser sends it, must stay on
// this site's origin (the local server only ever sees requests to itself, so it cannot show a leak).
const browserRequests = [];
const watchRequests = (context) => context.on('request', (request) => browserRequests.push(request.url()));
function assertSameOrigin(phase) {
  const own = new URL(site).origin;
  const foreign = browserRequests.filter((url) => !/^(data|blob):/.test(url) && new URL(url).origin !== own);
  assert.deepEqual(foreign, [], `${phase}: no request may reach another origin`);
}

try {
  {
    const headers = true;
    const mode = 'header';
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const attempts = [];
    let phase = 'load';
    context.on('request', (request) => attempts.push({ phase, method: request.method(), url: request.url() }));
    const page = await context.newPage();
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    const diagnostics = { pageErrors, console: [], failedRequests: [], workers: [] };
    page.on('console', (message) => {
      diagnostics.console.push({ type: message.type(), text: message.text(), location: message.location() });
    });
    context.on('requestfailed', (request) => {
      diagnostics.failedRequests.push({ url: request.url(), failure: request.failure() });
    });
    page.on('worker', (worker) => {
      const state = { url: worker.url(), closed: false };
      diagnostics.workers.push(state);
      worker.on('close', () => { state.closed = true; });
    });
    page.setDefaultTimeout(180_000);
    const response = await page.goto(`${site}/demo`);
    const csp = (await readFile('dist/client/_headers', 'utf8')).match(/^\/demo\n\s+Content-Security-Policy:\s*(.+)$/m)?.[1];
    assert(csp, 'The deployable policy for /demo must exist');
    assert.equal(response.headers()['content-security-policy'], csp);
    assert.match(csp, /connect-src 'none'/);
    await ready(page, diagnostics);
    const before = received.length;
    phase = 'operations';
    assert.equal((await page.locator('.drop-area .privacy').innerText()).replace(/\s*How to check\s*$/, '').trim(), 'Your files stay on this device. Nothing is uploaded: the comparison runs in your browser.');
    const explanation = page.locator('details.privacy-check');
    assert.equal(await explanation.getAttribute('open'), null, 'Privacy explanation starts folded');
    await page.getByRole('link', { name: 'How to check', exact: true }).click();
    assert.equal(await explanation.evaluate((element) => element.open), true, 'How to check opens the explanation');
    if (headers) {
      await explanation.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${root}/demo-privacy-check.png` });
    }
    await explanation.locator('summary').click();
    await context.setOffline(true);
    const files = [
      { path: '../examples/real/fixtures/paper.msoffice-blank-line.docx', time: 1 },
      { path: '../examples/real/paper.docx', time: Date.now() },
    ];
    const transfer = await page.evaluateHandle((files) => {
      const transfer = new DataTransfer();
      for (const file of files) transfer.items.add(new File([Uint8Array.from(file.bytes)], file.name, { lastModified: file.time }));
      return transfer;
    }, await Promise.all(files.map(async ({ path, time }) => ({ name: basename(path), bytes: [...await readFile(path)], time }))));
    await page.locator('#files-drop').dispatchEvent('drop', { dataTransfer: transfer });
    await transfer.dispose();
    await settled(page, 'results');
    assert.match(await page.locator('#comparison-summary').textContent(), /empty line.*Throughout this document/);
    await page.locator('#signing > summary').click();
    await page.locator('#author').fill('Offline reviewer');
    await page.locator('#sign').click();
    await settled(page, 'signature-result');
    assert.equal(JSON.parse(await page.locator('#verify-report').textContent()).valid, true);
    const proof = await page.locator('#proof').textContent();
    await page.locator('details.verify-file > summary').click();
    await page.locator('#proof-file').setInputFiles({ name: 'offline-proof.json', mimeType: 'application/json', buffer: Buffer.from(proof) });
    await settled(page, 'saved-verify-details');
    assert.equal(JSON.parse(await page.locator('#saved-verify-report').textContent()).valid, true);
    // Keep the monitor armed through deferred callbacks after the visible result.
    await page.waitForTimeout(600);
    assert.deepEqual(attempts.filter((request) => request.phase === 'operations'), [], `${mode}: offline drop, compare, sign and verify make zero attempts, including workers`);
    assert.equal(received.length, before, `${mode}: server receives no file-operation requests`);
    if (headers) {
      await page.locator('#comparison-summary').scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${root}/demo-offline.png` });
    }
    await context.setOffline(false);
    phase = 'negative-csp-probes';
    const beforeProbes = received.length;
    const probes = [];
    const worker = page.workers()[0];
    assert(worker, 'The comparison worker must be available');
    for (const [name, target] of [['page', page], ['worker', worker]]) {
      for (const url of ['https://example.com/privacy-probe', `${site}/demo/privacy-probe`]) {
        const result = await blockedFetch(target, url);
        assert.equal(result.rejected, true, `${mode}: ${name} fetch is rejected`);
        assert(result.violations.some((event) => event.directive === 'connect-src'), `${mode}: ${name} fetch reports a CSP connect-src violation: ${JSON.stringify(result)}`);
        probes.push({ target: name, url, ...result });
      }
    }
    assert.equal(received.length, beforeProbes, `${mode}: blocked self-origin fetch never reaches the static server`);
    assert.deepEqual(pageErrors, [], `${mode}: no uncaught page errors`);
    await writeFile(`${root}/demo-privacy-${mode}.json`, JSON.stringify({ csp, attempts, probes, received: received.slice() }, null, 2));
    console.log('PASS privacy /demo: offline drop, compare, sign and verify; zero post-load requests; page and worker fetch blocked for self and third party');
    await context.close();

    // /flow: the workbook opens in real GridCraft (page host mode); a save returns like an upload.
    {
      const flow = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      watchRequests(flow);
      const flowPage = await flow.newPage();
      const flowErrors = [];
      flowPage.on('pageerror', (error) => flowErrors.push(error.message));
      flowPage.setDefaultTimeout(180_000);
      const flowResponse = await flowPage.goto(`${site}/flow`);
      const flowCsp = flowResponse.headers()['content-security-policy'];
      assert(flowCsp && /connect-src 'self'/.test(flowCsp), `/flow must allow same-origin connections for the editors: ${flowCsp}`);
      assert(!/connect-src 'none'/.test(flowCsp), '/flow must not forbid same-origin connections');
      await flowPage.waitForFunction(() => document.getElementById('status').textContent.startsWith('Ready.') || !document.getElementById('error').hidden);
      assert.equal(await flowPage.locator('#error').isVisible(), false, await flowPage.locator('#error').textContent());
      // Open the bundled workbook and the real GridCraft editor beside the table preview.
      await flowPage.locator('#sample').click();
      await flowPage.waitForFunction(() => !document.getElementById('sample').disabled);
      await flowPage.locator('#open-gridcraft').click();
      await flowPage.locator('#gridcraft-editor .craft-editor canvas').waitFor({ timeout: 120_000 });
      await flowPage.waitForFunction(() => document.querySelector('#gridcraft-editor canvas').dataset.host === 'page');
      assert.equal(await flowPage.locator('iframe').count(), 0, 'The editor runs in the page, not an iframe');
      assertSameOrigin('flow editor load');
      // The real editor must actually open the demo's workbook, not just appear.
      await flowPage.waitForFunction(() => !document.getElementById('status').textContent.startsWith('Opening the workbook in GridCraft'), undefined, { timeout: 120_000 });
      assert.equal(await flowPage.locator('#error').isVisible(), false, await flowPage.locator('#error').textContent());
      // A workbook save from the editor must flow back exactly like an uploaded workbook:
      // dispatch the same non-bubbling event the app itself sends on the canvas.
      // The bytes are a real workbook (the edited fixture the flow check uses), so the demo accepts it.
      const saved = [...await readFile('../examples/hello-table/finance-edited.xlsx')];
      await flowPage.evaluate(async (saved) => {
        const canvas = document.querySelector('#gridcraft-editor canvas');
        const bytes = Uint8Array.from(saved);
        canvas.dispatchEvent(new CustomEvent('gridcraft:save', { detail: { name: 'finance.xlsx', bytes: bytes.buffer } }));
      }, saved);
      await flowPage.waitForFunction(() => document.getElementById('workbook-name').textContent.includes('saved in GridCraft'), undefined, { timeout: 60_000 });
      assert.equal(await flowPage.locator('#error').isVisible(), false, await flowPage.locator('#error').textContent());
      assertSameOrigin('flow editor save');
      assert.deepEqual(flowErrors, [], 'no uncaught page errors on /flow');
      console.log('PASS privacy /flow: editor loads from this origin only; an editor save returns through the demo without leaving the device');
      await flow.close();
    }

    // /office: open the desk, load an editor, save a file onto the desk, download it.
    {
      const office = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      watchRequests(office);
      const officePage = await office.newPage();
      const officeErrors = [];
      officePage.on('pageerror', (error) => officeErrors.push(error.message));
      officePage.setDefaultTimeout(180_000);
      const officeResponse = await officePage.goto(`${site}/office`);
      const officeCsp = officeResponse.headers()['content-security-policy'];
      assert(officeCsp && /connect-src 'self'/.test(officeCsp), `/office must allow same-origin connections for the editors: ${officeCsp}`);
      await officePage.locator('#office-files li').waitFor({ timeout: 5_000 }).catch(() => { /* An empty desk is fine. */ });
      await officePage.locator('#tab-gridcraft').click();
      await officePage.locator('#gridcraft_canvas').waitFor({ timeout: 120_000 });
      assert.equal(await officePage.locator('.office-editor-stage iframe').count(), 0, 'No iframe hosts the editor');
      assertSameOrigin('office editor load');
      // A save through the app's event lands on the desk in this browser's storage.
      await officePage.evaluate(async () => {
        const canvas = document.getElementById('gridcraft_canvas');
        const bytes = new Uint8Array([80, 75, 3, 4]);
        canvas.dispatchEvent(new CustomEvent('gridcraft:save', { detail: { name: 'desk-check.xlsx', bytes: bytes.buffer } }));
      });
      await officePage.locator('#office-files li').filter({ hasText: 'desk-check.xlsx' }).waitFor({ timeout: 30_000 });
      assertSameOrigin('office save to desk');
      assert.deepEqual(officeErrors, [], 'no uncaught page errors on /office');
      console.log('PASS privacy /office: editor save lands on the desk in browser storage; every request stays on this origin');
      await office.close();
    }
  }
} finally {
  await browser.close();
  await new Promise((done) => server.close(done));
}
