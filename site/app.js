'use strict';
const nf = new Intl.NumberFormat('en-US');
const pf = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1, minimumFractionDigits: 1 });

const menu = document.querySelector('.menu');
const nav = document.querySelector('#nav');
menu.addEventListener('click', () => {
  const open = menu.getAttribute('aria-expanded') !== 'true';
  menu.setAttribute('aria-expanded', String(open));
  nav.classList.toggle('open', open);
});
nav.querySelectorAll('a').forEach(a => a.addEventListener('click', () => {
  menu.setAttribute('aria-expanded', 'false');
  nav.classList.remove('open');
}));

// Stagger index for the ladder fill animation.
document.querySelectorAll('.ladder tbody tr').forEach((row, r) => {
  row.querySelectorAll('td').forEach((cell, c) => {
    cell.style.setProperty('--r', r);
    cell.style.setProperty('--c', c);
  });
});

async function updateMetrics() {
  try {
    const response = await fetch('./progress.json');
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const m = await response.json();
    if (m.metric !== 'heuristic_member_name_match' || !Number.isFinite(m.percent)) throw new Error('unsupported schema');
    document.querySelector('#overall').textContent = pf.format(m.percent);
    document.querySelector('#overall-count').textContent = `${nf.format(m.matched)} of ${nf.format(m.total)}`;
  } catch (error) {
    // The page keeps its last published snapshot if the JSON is unavailable.
    console.warn('R-Rising metrics unavailable:', error.message);
  }
}
updateMetrics();

const shots = {
  sceneNative: { path: 'assets/screenshots/native-original-scene.png', alt: 'A mountain scene from V Rising drawn by the native Rust renderer at the entities’ original positions', caption: 'One original scene, 9,474 mesh instances at their stored positions, native on macOS / Metal. No lighting parity; grass shows one texture-array layer; fog sheets are left out.' },
  sceneBrowser: { path: 'assets/screenshots/browser-original-scene.png', alt: 'The same V Rising scene drawn in Chrome through WebGPU', caption: 'The same scene in Chrome through WebAssembly and WebGPU, from an earlier build: it still shows white patches and an opaque fog cylinder that the native image no longer has.' },
  gridNative: { path: 'assets/screenshots/native-content.png', alt: 'Original V Rising models arranged in a demonstration grid by the native renderer', caption: '48 original models and 26 textures in a demonstration grid, native on macOS / Metal. The arrangement is not the game world.' },
  gridBrowser: { path: 'assets/screenshots/browser-webgpu.png', alt: 'The same model grid in Chrome through WebGPU', caption: 'The same grid in Chrome through WebGPU.' }
};
const tabs = [...document.querySelectorAll('[data-shot]')];
function show(tab) {
  const shot = shots[tab.dataset.shot];
  for (const t of tabs) {
    const on = t === tab;
    t.classList.toggle('active', on);
    t.setAttribute('aria-selected', String(on));
    t.tabIndex = on ? 0 : -1;
  }
  const img = document.querySelector('#shot-image');
  img.src = shot.path;
  img.alt = shot.alt;
  document.querySelector('#shot-link').href = shot.path;
  document.querySelector('#shot-caption').textContent = shot.caption;
  document.querySelector('#stage').setAttribute('aria-labelledby', tab.id);
}
for (const tab of tabs) {
  tab.addEventListener('click', () => show(tab));
  tab.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const i = tabs.indexOf(tab);
    const next = event.key === 'Home' ? tabs[0] : event.key === 'End' ? tabs[tabs.length - 1] : tabs[(i + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length];
    next.focus();
    show(next);
  });
}

for (const button of document.querySelectorAll('[data-copy]')) {
  button.addEventListener('click', async () => {
    const target = document.getElementById(button.dataset.copy);
    const announce = document.querySelector('#announce');
    try {
      await navigator.clipboard.writeText(target.textContent);
      button.textContent = 'Copied';
      announce.textContent = 'Commands copied';
    } catch {
      const range = document.createRange();
      range.selectNodeContents(target);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = 'Selected';
      announce.textContent = 'Commands selected, press copy to finish';
    }
    setTimeout(() => { button.textContent = 'Copy'; }, 2500);
  });
}
