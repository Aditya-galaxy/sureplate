// Records the demo video in a real browser: a device owner signs in, a deadline is missed,
// and they click Collect. Every step is a real testnet transaction.
//
//   node video/record.mjs
//
// Narration: run video/voice.sh first (Google Cloud TTS, Chirp 3 HD); scenes are timed to the clips.
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
// The viewer is signed in as the BUYER: the video shows a resale, then the new owner collecting.
const OWNER_PK = E.BUYER_CLAIM_PK || 'ed25519:6EB9e8Mt9BVG8B5RhD2AtonQacyhtYtsMs58A3Rpgp75';
const OWNER_SK = readFileSync(E.CLAIM_KEY_FILE || join(homedir(), '.near-credentials/testnet/pb-alice.claim-key.txt'), 'utf8').trim();
const WINDOW = Number(E.WINDOW || 45), PORT = 4174;
const run = new Date().toISOString().replace(/\D/g, '').slice(4, 14);
const LINE = `cam-s2-${run}`, TOKEN = `CAM-S2-${run}`;

const evidence = [];
const sh = cmd => { const out = sh0(cmd); for (const m of out.matchAll(/✓[^ ]* (\S+) → (\S+).*?txns\/(\w+)/g)) evidence.push([m[1], m[2], m[3]]); return out; };
const sh0 = cmd => execFileSync('bash', ['-c', `cd "${ROOT}" && source scripts/lib.sh && ${cmd}`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
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
}, { acct: BUYER, pk: OWNER_PK, sk: OWNER_SK });
const page = await ctx.newPage();
page.on('pageerror', e => log('PAGE ERROR ' + e.message));

const VOICE = join(ROOT, 'video/voice');
const clipLen = k => Number(execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', join(VOICE, `${k}.wav`)], { encoding: 'utf8' }));
const segments = [];                       // [file, seconds] in video time
const spoken = [];                         // [clip, start in video time]
let recording = false, n = 0, last = Date.now(), speed = 1;   // speed > 1 fast-forwards waiting on the chain
const videoNow = () => segments.reduce((t, s) => t + s[1], 0) + (recording ? (Date.now() - last) / 1000 / speed : 0);
let busyUntil = 0;                         // video time at which the current voice line ends
const say = async k => {                   // start a voice line now; never talk over the previous one
  while (recording && videoNow() < busyUntil) await sleep(40);
  const t = Math.max(videoNow(), busyUntil) + 0.15; spoken.push([k, t]); busyUntil = t + clipLen(k) + 0.25; log(`say ${k} @ ${t.toFixed(1)}s`);
};
const spokenOut = async () => { while (videoNow() < busyUntil) await sleep(40); };

const card = async (name, voice) => {
  await page.goto(`http://localhost:${PORT}/video/composite.html?card=${name}`); await sleep(400);
  await page.screenshot({ path: join(OUT, 'frames', `${name}.jpg`), type: 'jpeg', quality: 92 });
  const t = videoNow() + 0.5; spoken.push([voice, t]); const d = 0.5 + clipLen(voice) + 0.7;
  segments.push([`${name}.jpg`, d]); busyUntil = t + clipLen(voice) + 0.2;
};
await card('intro', 'intro');

await page.goto(`http://localhost:${PORT}/docs/index.html?token=${TOKEN}&embed=1`);
await page.waitForSelector('.banner'); await page.waitForSelector('#bal');
await page.addStyleTag({ content: `
  #cap { position: fixed; left: 0; right: 0; bottom: 0; padding: 15px 28px; background: rgba(6,8,10,.95); border-top: 1px solid #26303a;
    font: 600 21px/1.3 -apple-system, sans-serif; color: #fff; z-index: 9; text-align: center; }
  #cap small { display: block; font: 400 13.5px -apple-system, sans-serif; color: #8b98a5; margin-top: 3px; }
  #cur { position: fixed; width: 22px; height: 22px; border-radius: 50%; background: rgba(255,255,255,.88); border: 2px solid #0a0d10; z-index: 10;
    left: 1180px; top: 430px; transition: left .9s ease, top .9s ease, transform .15s; box-shadow: 0 2px 10px rgba(0,0,0,.6); pointer-events: none; }
  .toast { bottom: 92px !important; } .wrap { padding-bottom: 120px !important; }` });
await page.evaluate(() => { document.body.insertAdjacentHTML('beforeend', '<div id="cap"></div><div id="cur"></div>'); });
const cap = (t, sub = '') => page.evaluate(([t, sub]) => { document.getElementById('cap').innerHTML = t + (sub ? `<small>${sub}</small>` : ''); }, [t, sub]);

await cap('Bob owns an Acme Cam S2. Its passport NFT holds a 2 NEAR bond from the maker.', 'real transactions on NEAR testnet · you are watching as Alice, who is about to buy it');
recording = true; last = Date.now();
const loop = (async () => {
  while (recording) {
    const f = `f${String(n++).padStart(5, '0')}.jpg`;
    await page.screenshot({ path: join(OUT, 'frames', f), type: 'jpeg', quality: 88 });
    const now = Date.now(); segments.push([f, (now - last) / 1000 / speed]); last = now;
    await sleep(50);
  }
})();

await say('own'); await spokenOut();
await cap('A critical vulnerability is reported against the camera…');
await say('adv');
speed = 3;
sh(`call ${REPORTER} file_advisory '{"line_id":"${LINE}","reference":"CVE-2022-37434","component":"pkg:generic/zlib@1.2.11","severity":"Critical"}' "0.1 NEAR"`);
await page.waitForSelector('.p-pending', { timeout: 30000 });
speed = 1; await sleep(100);
await cap(`The maker promised a fix within ${WINDOW} seconds. The clock is running.`, 'a real product line would promise days, not seconds');
await page.locator('.adv').first().scrollIntoViewIfNeeded();
await spokenOut();
await page.evaluate(() => window.scrollTo({ top: 0 }));
await cap('Bob sells the camera to Alice. The passport, and the bond inside it, go with it.', 'a standard NEP-171 nft_transfer');
await say('resale');
speed = 2;
sh(`call ${OWNER} nft_transfer '{"receiver_id":"${BUYER}","token_id":"${TOKEN}"}' "1 yoctoNEAR"`);
await page.waitForFunction(b => document.querySelector('.grid .v')?.textContent.includes(b), BUYER, { timeout: 30000 });
speed = 1; await spokenOut();
await cap('Alice now owns the device. The maker is still on the clock.', 'fast-forward 3x');
speed = 3;
await page.waitForSelector('.collect', { timeout: (WINDOW + 60) * 1000 });
speed = 1; log('breach visible');
await cap('Deadline missed. No patch. The bond now owes the current owner: Alice.');
await say('miss'); await sleep(1500);
await page.locator('.collect').scrollIntoViewIfNeeded();
await spokenOut();
const box = await page.locator('.collect').boundingBox();
await page.evaluate(([x, y]) => { const c = document.getElementById('cur'); c.style.left = x + 'px'; c.style.top = y + 'px'; }, [box.x + box.width / 2 - 11, box.y + box.height / 2 - 11]);
await cap('Alice, the new owner, clicks Collect.');
await say('click'); await sleep(1500);
await page.evaluate(() => { document.getElementById('cur').style.transform = 'scale(.7)'; });
await page.locator('.collect').click();
await page.waitForSelector('.toast:not([hidden])', { timeout: 60000 });
const claimTx = (await page.locator('.toast a').getAttribute('href')).split('/').pop();
evidence.push([BUYER, 'claim (in the browser)', claimTx]);
log('claim toast: ' + (await page.locator('.toast').innerText()).slice(0, 80));
await page.evaluate(() => { document.getElementById('cur').style.left = '1400px'; });
await cap('Paid: 0.9958 NEAR, half the bond, from the maker\u2019s escrow to Alice\u2019s wallet.', `transaction ${claimTx.slice(0, 12)}… on NEAR testnet`);
await say('paid');
await sleep(2500);
await page.evaluate(() => window.scrollTo({ top: 0 }));
await page.waitForSelector('.bal.up', { timeout: 30000 }).catch(() => log('balance flash not seen'));
await spokenOut(); await sleep(1200);
recording = false; await loop;
await card('outro', 'outro');
await browser.close(); server.kill();

const list = segments.map(([f, d]) => `file 'frames/${f}'\nduration ${Math.max(d, 0.04).toFixed(3)}`).join('\n') + `\nfile 'frames/${segments.at(-1)[0]}'\n`;
writeFileSync(join(OUT, 'list.txt'), list);
const total = segments.reduce((t, s) => t + s[1], 0);
const ins = spoken.flatMap(([k]) => ['-i', join(VOICE, `${k}.wav`)]);
const filt = spoken.map(([, t], i) => `[${i + 1}:a]adelay=${Math.round(t * 1000)}:all=1[a${i}]`).join(';') +
  `;${spoken.map((_, i) => `[a${i}]`).join('')}amix=inputs=${spoken.length}:normalize=0,apad=whole_dur=${total.toFixed(2)},loudnorm=I=-16:TP=-1.5[a]`;
execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', join(OUT, 'list.txt'), ...ins,
  '-filter_complex', `[0:v]fps=30,format=yuv420p[v];${filt}`, '-map', '[v]', '-map', '[a]',
  '-c:v', 'libx264', '-crf', '21', '-c:a', 'aac', '-b:a', '160k', '-ar', '48000', '-t', total.toFixed(2), '-movflags', '+faststart', join(OUT, 'sureplate-demo.mp4')]);
writeFileSync(join(OUT, 'evidence.md'), evidence.map(([who, what, tx]) => `| ${who} | \`${what}\` | [${tx.slice(0, 10)}…](https://testnet.nearblocks.io/txns/${tx}) |`).join('\n') + '\n');
log(`done: ${n} frames, ${total.toFixed(1)}s → video/out/sureplate-demo.mp4 · passport ${TOKEN}`);
