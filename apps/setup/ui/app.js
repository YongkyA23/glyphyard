'use strict';
// Glyphyard Setup UI. Contract: docs/setup.md. No framework, no build step.

const $ = (s) => document.querySelector(s);
const T = window.__TAURI__ || mockTauri();
const invoke = T.core.invoke;
const reduced = matchMedia('(prefers-reduced-motion: reduce)');

const STEPS = {
  install: [['copy', 'Copying Glyphyard'], ['webview2', 'Checking WebView2'], ['plugins', 'Preparing Adobe plug-ins'], ['shortcuts', 'Creating shortcuts']],
  uninstall: [['fonts', 'Turning fonts off'], ['files', 'Removing files'], ['registry', 'Cleaning up the registry']],
};

let info = null;
let current = null;   // visible <section>
let busy = false;     // install/uninstall/close_app running: no quitting
let job = 'install';  // 'install' | 'uninstall' (what the progress screen is doing)
let stepIdx = 0;
const opts = { dir: '', startMenu: true, desktop: false, startAtLogin: true };

// ---------- screens ----------
function show(name) {
  const next = $('#s-' + name);
  if (next === current) return;
  const prev = current;
  const hadFocus = prev && prev.contains(document.activeElement);
  current = next;
  next.hidden = false;
  next.classList.add('in');
  if (prev) {
    prev.classList.remove('in');
    setTimeout(() => { if (prev !== current) prev.hidden = true; }, reduced.matches ? 0 : 200);
  }
  // Keep keyboard users on the new screen; mouse users get no ring (focus-visible heuristic).
  if (hadFocus) (next.querySelector('[data-primary]:not(:disabled)') || next.querySelector('button'))?.focus();
}
const home = () => (info.mode === 'update' ? 'update' : 'welcome');

function requestClose() {
  if (busy) return; // ponytail: mid-install quit is simply refused; the progress screen has no close control.
  invoke('quit');
}

// ---------- info ----------
function fill() {
  const v = { ...info, sizeMb: Math.round(info.sizeMb) };
  document.querySelectorAll('[data-f]').forEach((el) => { el.textContent = v[el.dataset.f] ?? ''; });
  opts.dir = info.defaultDir;
  syncOpts();

  const n = info.fontsActive;
  $('#un-fonts').textContent =
    n == null ? 'Any fonts Glyphyard turned on are turned off, cleanly.'
    : n === 0 ? 'No fonts are turned on by Glyphyard right now, so none need turning off.'
    : n === 1 ? 'The 1 font Glyphyard turned on is turned off, cleanly.'
    : `The ${n} fonts Glyphyard turned on are turned off, cleanly.`;
}

// ---------- options ----------
function syncOpts() {
  $('#opt-dir').value = opts.dir;
  document.querySelectorAll('[data-opt]').forEach((b) => b.setAttribute('aria-pressed', String(opts[b.dataset.opt])));
}
document.querySelectorAll('[data-opt]').forEach((b) => b.addEventListener('click', () => {
  opts[b.dataset.opt] = !opts[b.dataset.opt];
  syncOpts();
}));
$('#opt-change').addEventListener('click', async () => {
  const dir = await invoke('pick_dir', { current: opts.dir }).catch(() => null);
  if (dir) { opts.dir = dir; syncOpts(); }
});

// ---------- progress ----------
function startProgress(kind) {
  job = kind;
  busy = true;
  stepIdx = 0;
  const scr = $('#s-progress');
  scr.dataset.job = kind;
  scr.classList.remove('failed');
  $('#p-eyebrow').textContent = kind === 'uninstall' ? 'Removing' : info.mode === 'update' ? 'Updating' : 'Installing';
  $('#p-bar').setAttribute('aria-label', kind === 'uninstall' ? 'Removal progress' : 'Install progress');
  $('#p-steps').replaceChildren(...STEPS[kind].map(([, label]) => {
    const li = document.createElement('li');
    li.innerHTML = '<span class="mk"><span class="m-done">✓</span><span class="m-active"></span><span class="m-pending"></span><span class="m-failed">!</span></span><span></span>';
    li.lastChild.textContent = label;
    return li;
  }));
  document.querySelectorAll('#p-glyphs .gl').forEach((g) => { g.hidden = true; });
  $('#p-err').hidden = true;
  $('#p-actions').hidden = true;
  $('#p-note').hidden = false;
  setPct(0);
  paintSteps();
  show('progress');
}

function setPct(p) {
  p = Math.max(0, Math.min(100, Math.round(p)));
  $('#p-pct').textContent = p;
  $('#p-bar').setAttribute('aria-valuenow', p);
  $('#p-fill').style.width = p + '%';
  // Letters drop onto the shelf as pct crosses 4, 21, 38, 55, 72, 89 (same thresholds as the mockup).
  document.querySelectorAll('#p-glyphs .gl').forEach((g, i) => { if (p >= i * 17 + 4) g.hidden = false; });
}

function paintSteps(allDone = false, failed = false) {
  [...$('#p-steps').children].forEach((li, i) => {
    li.className = allDone || i < stepIdx ? 'done' : i === stepIdx ? (failed ? 'failed' : 'active') : 'pending';
  });
}

T.event.listen('progress', ({ payload }) => {
  if (!busy || current !== $('#s-progress')) return;
  setPct(payload.pct);
  const i = STEPS[job].findIndex(([k]) => k === payload.step);
  if (i > stepIdx) stepIdx = i; // never move backwards
  paintSteps(payload.pct >= 100);
});

function finishProgress() {
  setPct(100);
  paintSteps(true);
  busy = false;
}

function failed(e) {
  busy = false;
  const code = e && e.code;
  if (code === 'app_running') {
    $('#r-lead').textContent = job === 'uninstall'
      ? 'Setup needs to remove files Glyphyard is using. It closes Glyphyard first, then turns your fonts off cleanly.'
      : 'Setup needs to replace files Glyphyard is using. Your fonts stay turned on while it restarts. Nothing in your library changes.';
    show('running');
    return;
  }
  $('#s-progress').classList.add('failed');
  $('#p-eyebrow').textContent = job === 'uninstall' ? 'Couldn’t remove' : 'Couldn’t finish';
  paintSteps(false, true);
  $('#p-err').textContent = (e && e.message) || String(e);
  $('#p-err').hidden = false;
  $('#p-note').hidden = true;
  $('#p-actions').hidden = false;
  const retry = $('#p-retry');
  retry.textContent = code === 'invalid_dir' ? 'Choose another folder' : 'Try again';
  retry.dataset.action = code === 'invalid_dir' ? 'options' : 'retry';
  if (current.contains(document.activeElement) || document.activeElement === document.body) retry.focus();
}

const retryJob = () => (job === 'uninstall' ? runUninstall() : runInstall());
$('#p-retry').addEventListener('click', (e) => (e.currentTarget.dataset.action === 'options' ? show('options') : retryJob()));

// ---------- install / update ----------
async function runInstall() {
  startProgress('install');
  try {
    await invoke('install', { ...opts });
  } catch (e) {
    return failed(e);
  }
  finishProgress();
  prepareDone();
  setTimeout(() => show('done'), 700); // let the last check pop before moving on
}

function prepareDone() {
  const update = info.mode === 'update';
  $('#d-lead').textContent = update
    ? 'Your library, active fonts, collections and settings are just as you left them.'
    : 'When it opens, it asks what to call you. That’s the only setup. Then add a font folder and you’re done.';
  // The login choice was applied during install; shown read-only here (no command to change it from Setup).
  $('#d-login').hidden = update;
  const sw = $('#d-login-sw');
  sw.classList.toggle('on', opts.startAtLogin);
  sw.setAttribute('aria-label', opts.startAtLogin ? 'On' : 'Off');
}

$('#welcome-install').addEventListener('click', runInstall);
$('#welcome-options').addEventListener('click', () => show('options'));
$('#opt-install').addEventListener('click', runInstall);
$('#u-update').addEventListener('click', runInstall);
document.querySelectorAll('[data-go]').forEach((b) => b.addEventListener('click', () => show(home())));

$('#r-retry').addEventListener('click', retryJob);
$('#r-close').addEventListener('click', async (e) => {
  const btn = e.currentTarget;
  const label = btn.textContent;
  busy = true;
  btn.disabled = $('#r-retry').disabled = true;
  btn.textContent = 'Closing…';
  await invoke('close_app').catch(() => {});
  btn.disabled = $('#r-retry').disabled = false;
  btn.textContent = label;
  retryJob();
});

$('#d-open').addEventListener('click', async (e) => {
  e.currentTarget.disabled = true;
  await invoke('launch_app').catch(() => { e.currentTarget.disabled = false; });
});

// ---------- uninstall ----------
$('#un-wipe').addEventListener('change', (e) => { $('#un-warn').hidden = !e.target.checked; });
$('#un-remove').addEventListener('click', runUninstall);

async function runUninstall() {
  const wipeData = $('#un-wipe').checked;
  startProgress('uninstall');
  let r;
  try {
    r = await invoke('uninstall', { wipeData });
  } catch (e) {
    return failed(e);
  }
  finishProgress();
  $('#rm-off').textContent = r.fontsTurnedOff;
  const left = $('#rm-left');
  left.textContent = r.leftoverEntries;
  left.className = 'w600 num ' + (r.leftoverEntries ? 'bad' : 'ok');
  $('#rm-note').textContent = !wipeData
    ? 'Your library data was kept, so reinstalling picks up where you left off.'
    : r.fontsKeptIn
      ? `Your library data was deleted. Fonts you imported into Glyphyard were kept in ${r.fontsKeptIn}.`
      : 'Your library data was deleted too, so a reinstall starts fresh.';
  setTimeout(() => show('removed'), 500);
}

// ---------- global: close, keyboard, drag ----------
document.addEventListener('click', (e) => { if (e.target.closest('[data-close]')) requestClose(); });
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    e.preventDefault();
    requestClose();
  } else if (e.key === 'Enter' && !e.target.closest?.('button, a, input[type=checkbox]')) {
    const p = current && [...current.querySelectorAll('[data-primary]')].find((b) => !b.disabled && b.offsetParent);
    if (p) { e.preventDefault(); p.click(); }
  }
});
// Tauri only drags when the pressed element itself carries the attribute, so mark the whole dark panel.
document.querySelectorAll('.side, .side *').forEach((el) => el.setAttribute('data-tauri-drag-region', ''));

(async () => {
  info = await invoke('info');
  fill();
  show(info.mode === 'uninstall' ? 'uninstall' : home());
})();

// ---------- browser mock (no Tauri): ?mode=install|update|uninstall&fail=app_running|io|invalid_dir&fonts=N ----------
function mockTauri() {
  document.documentElement.classList.add('mock');
  const q = new URLSearchParams(location.search);
  const mode = q.get('mode') || 'install';
  const fonts = q.has('fonts') ? (q.get('fonts') === 'null' ? null : Number(q.get('fonts'))) : 86;
  let fail = q.get('fail');
  let emit = () => {};
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const takeFail = (code) => { if (fail !== code) return false; fail = null; return true; };

  async function run(plan) { // plan: [[step, endPct], ...]
    for (let pct = 0; pct < 100; pct += 2 + Math.round(Math.random() * 2)) {
      emit({ payload: { pct, step: plan.find(([, end]) => pct < end)[0] } });
      if (pct >= 50 && takeFail('io')) throw { code: 'io', message: 'Couldn’t write typeshelf.exe: the disk is full.' };
      await wait(70);
    }
    emit({ payload: { pct: 100, step: plan[plan.length - 1][0] } });
  }
  const cmds = {
    info: () => ({
      mode, version: '0.2.0', installedVersion: mode === 'install' ? null : '0.1.0',
      defaultDir: 'C:\\Users\\budi\\AppData\\Local\\Programs\\Typeshelf',
      serverUrl: 'fonts.studio-budi.id', sizeMb: 19.6, fontsActive: mode === 'uninstall' ? fonts : null,
    }),
    pick_dir: async () => { await wait(300); return 'D:\\Apps\\Typeshelf'; },
    install: async () => {
      await wait(200);
      if (takeFail('app_running')) throw { code: 'app_running', message: 'Glyphyard is running' };
      if (takeFail('invalid_dir')) throw { code: 'invalid_dir', message: 'Setup can’t write to that folder. Pick one inside your user folder.' };
      await run([['copy', 45], ['webview2', 62], ['plugins', 84], ['shortcuts', 101]]);
    },
    uninstall: async ({ wipeData }) => {
      await wait(200);
      if (takeFail('app_running')) throw { code: 'app_running', message: 'Glyphyard is running' };
      await run([['fonts', 50], ['files', 85], ['registry', 101]]);
      const fontsKeptIn = wipeData ? 'C:\\Users\\budi\\AppData\\Roaming\\app.typeshelf.desktop' : null;
      return { fontsTurnedOff: fonts ?? 0, leftoverEntries: 0, fontsKeptIn };
    },
    close_app: () => wait(800),
    launch_app: () => {},
    quit: () => {},
  };
  return {
    core: { invoke: async (cmd, args) => { console.log('[mock invoke]', cmd, JSON.stringify(args ?? {})); return cmds[cmd](args); } },
    event: { listen: async (_name, h) => { emit = h; return () => {}; } },
  };
}
