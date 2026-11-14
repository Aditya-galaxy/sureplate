// Records the demo video in a real browser: a device owner signs in, a deadline is missed,
// and they click Collect. Every step is a real testnet transaction.
//
//   node video/record.mjs
//
// Needs: Google Chrome, ffmpeg, near-cli-rs, the demo accounts in ~/.near-credentials, and a
// function-call key for the owner (claim only) in CLAIM_KEY_FILE. That key is what the wallet
// sign-in redirect would have put in localStorage; injecting it skips the redirect on camera.
import { chromium } from 'playwright-core';
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, rmSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUT = join(ROOT, 'video/out'); rmSync(OUT, { recursive: true, force: true }); mkdirSync(join(OUT, 'frames'), { recursive: true });
const E = process.env;
const CONTRACT = E.CONTRACT || 'sureplate.testnet', MAKER = E.MAKER || 'acme-cam.testnet', OWNER = E.OWNER || 'pb-bob.testnet',
  BUYER = E.BUYER || 'pb-alice.testnet', REPORTER = E.REPORTER || 'pb-reporter.testnet', ARBITER = E.ARBITER || 'pb-cert.testnet';
const OWNER_PK = E.OWNER_CLAIM_PK || 'ed25519:JCarukvEBoiyM35X2wPbWBXhJ4UGd7ArmFeQaYSydBcG';
const OWNER_SK = readFileSync(E.CLAIM_KEY_FILE || join(homedir(), '.near-credentials/testnet/pb-bob.claim-key.txt'), 'utf8').trim();
const WINDOW = Number(E.WINDOW || 30), PORT = 4174;
const run = new Date().toISOString().replace(/\D/g, '').slice(4, 14);
const LINE = `cam-s2-${run}`, TOKEN = `CAM-S2-${run}`;

const sh = cmd => execFileSync('bash', ['-c', `cd "${ROOT}" && source scripts/lib.sh && ${cmd}`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
const sleep = ms => new Promise(r => setTimeout(r, ms));
const log = m => console.log(`[${new Date().toISOString().slice(11, 19)}] ${m}`);

log(`setup: line ${LINE}, unit ${TOKEN} for ${OWNER}`);
const sbom = execFileSync('shasum', ['-a', '256', join(ROOT, 'sbom/acme-cam-s2-1.4.0.cdx.json')], { encoding: 'utf8' }).split(' ')[0];
const until = Math.floor(Date.now() / 1000) + 5 * 365 * 86400;
sh(`call ${MAKER} create_line '{"line_id":"${LINE}","name":"Acme Cam S2","arbiter":"${ARBITER}","patch_window_sec":${WINDOW},"advisory_stake":"100000000000000000000000"}'`);
sh(`call ${MAKER} mint_unit '{"token_id":"${TOKEN}","line_id":"${LINE}","owner_id":"${OWNER}","sbom_sha256":"${sbom}","firmware":"1.4.0","support_until":${until}}' "2 NEAR"`);

const server = spawn('python3', ['-m', 'http.server', String(PORT), '--directory', ROOT], { stdio: 'ignore' });
await sleep(1000);
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 });
await ctx.addInitScript(({ acct, pk, sk }) => {
  localStorage.setItem('sureplate_wallet_auth_key', JSON.stringify({ accountId: acct, allKeys: [pk] }));
  localStorage.setItem(`near-api-js:keystore:${acct}:testnet`, sk);
}, { acct: OWNER, pk: OWNER_PK, sk: OWNER_SK });
const page = await ctx.newPage();
page.on('pageerror', e => log('PAGE ERROR ' + e.message));

const segments = [];                       // [file, seconds]
const card = async (name, secs) => {
  await page.goto(`http://localhost:${PORT}/video/composite.html?card=${name}`); await sleep(400);
  await page.screenshot({ path: join(OUT, 'frames', `${name}.jpg`), type: 'jpeg', quality: 92 });
  segments.push([`${name}.jpg`, secs]);
};
await card('intro', 4.5);

await page.goto(`http://localhost:${PORT}/docs/index.html?token=${TOKEN}`);
await page.waitForSelector('.badge'); await page.waitForSelector('#bal');
await page.addStyleTag({ content: `
  #cap { position: fixed; left: 0; right: 0; bottom: 0; padding: 16px 28px; background: rgba(6,8,10,.94); border-top: 1px solid #26303a;
    font: 600 22px/1.3 -apple-system, sans-serif; color: #fff; z-index: 9; text-align: center; }
  #cap small { display: block; font: 400 14px -apple-system, sans-serif; color: #8b98a5; margin-top: 3px; }
  #cur { position: fixed; width: 22px; height: 22px; border-radius: 50%; background: rgba(255,255,255,.85); border: 2px solid #0c0f12; z-index: 10;
    left: 1180px; top: 420px; transition: left .9s ease, top .9s ease, transform .15s; box-shadow: 0 2px 10px rgba(0,0,0,.6); pointer-events: none; }
  .toast { bottom: 96px !important; } .wrap { padding-bottom: 110px !important; } html { scroll-behavior: smooth; }` });
await page.evaluate(() => { document.body.insertAdjacentHTML('beforeend', '<div id="cap"></div><div id="cur"></div>'); });
const cap = (t, sub = '') => page.evaluate(([t, sub]) => { document.getElementById('cap').innerHTML = t + (sub ? `<small>${sub}</small>` : ''); }, [t, sub]);

let recording = true, n = 0, last = Date.now(), speed = 1;   // speed > 1 fast-forwards waiting on the chain
const loop = (async () => {
  while (recording) {
    const f = `f${String(n++).padStart(5, '0')}.jpg`;
    await page.screenshot({ path: join(OUT, 'frames', f), type: 'jpeg', quality: 88 });
    const now = Date.now(); segments.push([f, (now - last) / 1000 / speed]); last = now;
    await sleep(60);
  }
})();

await cap('Bob owns an Acme Cam S2. Its passport NFT holds a 2 NEAR bond from the maker.', 'real transactions on NEAR testnet · he is signed in with a key that can only call claim');
await sleep(5000);
await cap('A critical vulnerability is reported against the camera…');
speed = 3;
sh(`call ${REPORTER} file_advisory '{"line_id":"${LINE}","reference":"CVE-2022-37434","component":"pkg:generic/zlib@1.2.11","severity":"Critical"}' "0.1 NEAR"`);
await page.waitForSelector('.p-pending', { timeout: 30000 });
speed = 1; await sleep(100);
await cap(`The maker promised a fix within ${WINDOW} seconds. The clock is running.`, 'a real product line would promise days, not seconds · fast-forward 3x');
await sleep(3000); speed = 3;
await page.waitForSelector('.collect', { timeout: (WINDOW + 40) * 1000 });
speed = 1; log('breach visible');
await cap('Deadline missed. No patch. The bond now owes the owner.');
await page.locator('.collect').scrollIntoViewIfNeeded();
await sleep(2500);
const box = await page.locator('.collect').boundingBox();
await page.evaluate(([x, y]) => { const c = document.getElementById('cur'); c.style.left = x + 'px'; c.style.top = y + 'px'; }, [box.x + box.width / 2 - 11, box.y + box.height / 2 - 11]);
await sleep(1300);
await page.evaluate(() => { document.getElementById('cur').style.transform = 'scale(.7)'; });
await page.locator('.collect').click();
await cap('Bob clicks Collect.');
await page.waitForSelector('.toast:not([hidden])', { timeout: 60000 });
log('claim toast: ' + (await page.locator('.toast').innerText()).slice(0, 80));
await page.evaluate(() => { document.getElementById('cur').style.left = '1400px'; window.scrollTo({ top: 0 }); });
await cap('Paid. Half the bond moved from the maker’s escrow to Bob’s wallet.', 'watch the balance at the top right and the bond bar');
await page.waitForSelector('.bal.up', { timeout: 30000 }).catch(() => log('balance flash not seen'));
await sleep(5000);
await cap('Bob sells the camera to Alice. The passport, and the rest of the bond, go with it.');
speed = 2.5;
sh(`call ${OWNER} nft_transfer '{"receiver_id":"${BUYER}","token_id":"${TOKEN}"}' "1 yoctoNEAR"`);
await page.waitForFunction(b => document.querySelector('.grid .v')?.textContent.includes(b), BUYER, { timeout: 30000 });
speed = 1; await sleep(4000);
recording = false; await loop;
await card('outro', 5);
await browser.close(); server.kill();

const list = segments.map(([f, d]) => `file 'frames/${f}'\nduration ${Math.max(d, 0.04).toFixed(3)}`).join('\n') + `\nfile 'frames/${segments.at(-1)[0]}'\n`;
writeFileSync(join(OUT, 'list.txt'), list);
execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', join(OUT, 'list.txt'), '-vf', 'fps=30,format=yuv420p',
  '-c:v', 'libx264', '-crf', '21', '-movflags', '+faststart', join(OUT, 'sureplate-demo.mp4')]);
log(`done: ${n} frames → video/out/sureplate-demo.mp4 · passport ${TOKEN}`);
