// Local-only diagnostic: load an owner's export, send a real browser key press, and check stopping.
// Usage: node tools/check_browser_input.cjs <viewer-url> <local-scene.json> <output.png>
const { chromium } = require(process.env.RRISING_PLAYWRIGHT_MODULE || 'playwright');
const [url, scene, output] = process.argv.slice(2);
if (!url || !scene || !output) throw Error('Expected viewer URL, local scene and output PNG');
(async () => {
  const browser = await chromium.launch({ channel: 'chrome', headless: true,
    args: ['--enable-unsafe-webgpu', '--enable-dawn-features=allow_unsafe_apis'] });
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1050 }, deviceScaleFactor: 1 });
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
    await page.goto(`${url}?diagnostic=1`, { waitUntil: 'networkidle' });
    await page.waitForFunction(() => document.querySelector('#status').textContent.startsWith('Ready'));
    await page.locator('canvas').evaluate(canvas => { canvas.width = 1280; canvas.height = 720; });
    await page.locator('#scene').setInputFiles(scene);
    await page.waitForFunction(() => document.documentElement.dataset.loaded === 'true', null, { timeout: 60000 });
    const position = () => page.evaluate(() => Array.from(window.rrisingPlayerPosition()));
    const before = await position();
    if (before.length !== 3) throw Error('Export has no diagnostic player');
    await page.locator('canvas').click({position: {x: 600, y: 350}});
    await page.keyboard.down('w');
    await page.waitForTimeout(300);
    const walkFrame = await page.evaluate(() => window.rrisingPlayerAnimationFrame());
    await page.keyboard.up('w');
    await page.waitForTimeout(250);
    const after = await position();
    await page.waitForTimeout(250);
    const stopped = await position();
    const restFrame = await page.evaluate(() => window.rrisingPlayerAnimationFrame());
    if (!(after[2] > before[2])) throw Error(`W did not move forward: ${before} -> ${after}`);
    if (stopped.some((v, i) => v !== after[i])) throw Error(`Player did not stop: ${after} -> ${stopped}`);
    if (walkFrame < 0 || restFrame !== -1) throw Error(`Animation did not switch: walk=${walkFrame}, rest=${restFrame}`);
    if (errors.length) throw Error(errors.join('\n'));
    await page.locator('canvas').screenshot({path: output});
    console.log(JSON.stringify({browser: browser.version(), before, after, stopped, walkFrame, restFrame, errors}, null, 2));
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exit(1); });
