// Demo A on the site (/demo): files never leave the browser. Run on the box after the site build.
import { chromium } from '@playwright/test';
import { serve } from './serve.mjs';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, basename } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve('_shots');
const received = [];
const { server, origin: site } = await serve('dist/client', 0, (request) => received.push(request));
const origin = `${site}/demo`;
const csp = (await readFile('dist/client/_headers', 'utf8')).match(/^\/demo\n\s+Content-Security-Policy:\s*(.+)$/m)?.[1];
assert(csp, 'The deployable policy for /demo must exist');
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
    const response = await page.goto(origin);
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
      for (const url of ['https://example.com/privacy-probe', `${origin}/privacy-probe`]) {
        const result = await blockedFetch(target, url);
        assert.equal(result.rejected, true, `${mode}: ${name} fetch is rejected`);
        assert(result.violations.some((event) => event.directive === 'connect-src'), `${mode}: ${name} fetch reports a CSP connect-src violation: ${JSON.stringify(result)}`);
        probes.push({ target: name, url, ...result });
      }
    }
    assert.equal(received.length, beforeProbes, `${mode}: blocked self-origin fetch never reaches the static server`);
    assert.deepEqual(pageErrors, [], `${mode}: no uncaught page errors`);
    await writeFile(`${root}/demo-privacy-${mode}.json`, JSON.stringify({ csp, attempts, probes, received: received.slice() }, null, 2));
    console.log('PASS privacy: offline drop, compare, sign and verify; zero post-load requests; page and worker fetch blocked for self and third party');
    await context.close();
  }
} finally {
  await browser.close();
  await new Promise((done) => server.close(done));
}
