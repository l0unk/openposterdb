// Screenshots of docs/louk/badges.html at real pixel sizes (deviceScaleFactor 1), for the design review.
// Not part of CI. Uses the headless Chromium recipe from the fork's preflight report.
//
//   LD_LIBRARY_PATH=/home/dev/chromium-scratch/libs/usr/lib/x86_64-linux-gnu:/home/dev/chromium-scratch/libs/lib/x86_64-linux-gnu \
//   nice -n 19 timeout 600 node scripts/louk/gallery-shots.js [out-dir] [raw-dir]
//
// Writes: tv-row-<look>.png (1920 wide), tv-1080-<n>.png (1920x1080 viewport cuts), phone-<look>.png
// (412x915), hero-<title>-<col|row>-<look>.png (1280x576) and zoom-<title>-<look>.png (the bottom of
// a 240x360 poster card, enlarged 3x with nearest-neighbour, from the raw server render in raw-dir).
const path = require('path');
const fs = require('fs');
const { chromium } = require(process.env.PW_CORE || '/home/dev/chromium-scratch/node_modules/playwright-core');

const ROOT = path.resolve(__dirname, '..', '..');
const OUT = path.resolve(process.argv[2] || '/home/dev/opdb-fork/reports/P4.2-shots');
const RAW = path.resolve(process.argv[3] || path.join(process.env.HOME, 'opdb-dev/gallery/raw'));
const EXE = process.env.CHROMIUM || '/home/dev/.cache/ms-playwright/chromium_headless_shell-1148/chrome-linux/headless_shell';
const LOOKS = ['d', 's', 'm', 'a'];
const HERO_TITLES = ['kalki', 'modern-family'];
const ZOOM_TITLES = ['modern-family', 'godfather-2', 'kalki'];

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch({ executablePath: EXE, headless: true, args: ['--disable-gpu', '--no-zygote', '--renderer-process-limit=1'] });
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  await page.goto('file://' + path.join(ROOT, 'docs/louk/badges.html'));
  await page.waitForLoadState('load');
  const shot = async (sel, name) => { await (await page.$(sel)).screenshot({ path: path.join(OUT, name) }); };

  for (const v of LOOKS) await shot(`#tv-${v}`, `tv-row-${v}.png`);
  const top = await page.evaluate(() => document.querySelector('#tv-row').getBoundingClientRect().top + scrollY);
  for (let i = 0; i < 2; i++) {
    await page.screenshot({ path: path.join(OUT, `tv-1080-${i + 1}.png`), fullPage: true, clip: { x: 0, y: top + i * 840, width: 1920, height: 1080 } });
  }
  for (const v of LOOKS) await shot(`#phone-${v}`, `phone-${v}.png`);
  for (const t of HERO_TITLES) for (const variant of ['col', 'row']) for (const v of LOOKS) {
    if (v === 'a' && variant === 'row') continue; // identical to the column
    await shot(`#hero-${t}-${variant}-${v} .hero`, `hero-${t}-${variant}-${v}.png`);
  }

  // 3x pixel crops of the badge area of a 240x360 card (what a TV user sees, enlarged).
  for (const t of ZOOM_TITLES) for (const v of LOOKS) {
    const raw = ['jpg', 'png'].map(e => path.join(RAW, `${t}-poster-${v}.${e}`)).find(fs.existsSync);
    if (!raw) continue;
    const p = await ctx.newPage();
    // file:// images need a file:// page (about:blank may not read them)
    const tmp = path.join(require('os').tmpdir(), `opdb-zoom-${process.pid}.html`);
    fs.writeFileSync(tmp, `<body style="margin:0;background:#0b0b0f"><img id=c style="display:block;width:240px;height:360px;object-fit:cover" src="file://${raw}"></body>`);
    await p.goto('file://' + tmp);
    await p.waitForFunction(() => document.getElementById('c').complete);
    const buf = await p.screenshot({ clip: { x: 0, y: 230, width: 240, height: 130 } });
    await p.setContent(`<body style="margin:0;background:#0b0b0f"><img style="display:block;width:720px;height:390px;image-rendering:pixelated" src="data:image/png;base64,${buf.toString('base64')}"></body>`);
    await p.screenshot({ path: path.join(OUT, `zoom-${t}-${v}.png`), clip: { x: 0, y: 0, width: 720, height: 390 } });
    fs.unlinkSync(tmp);
    await p.close();
  }
  await browser.close();
  console.log('shots in', OUT);
})().catch(e => { console.error(e); process.exit(1); });
