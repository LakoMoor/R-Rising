'use strict';
const formatNumber = number => new Intl.NumberFormat('en-US').format(number);
const formatPercent = number => new Intl.NumberFormat('en-US', { maximumFractionDigits: 1, minimumFractionDigits: 1 }).format(number);
const menu = document.querySelector('.menu-toggle');
const navigation = document.querySelector('#navigation');
menu.addEventListener('click', () => {
  const open = menu.getAttribute('aria-expanded') !== 'true';
  menu.setAttribute('aria-expanded', String(open));
  navigation.classList.toggle('open', open);
});
navigation.querySelectorAll('a').forEach(link => link.addEventListener('click', () => {
  menu.setAttribute('aria-expanded', 'false');
  navigation.classList.remove('open');
}));

async function updateMetrics() {
  try {
    const response = await fetch('./progress.json');
    if (!response.ok) throw new Error(`Metrics: ${response.status}`);
    const metrics = await response.json();
    if (metrics.metric !== 'heuristic_member_name_match' || !Number.isFinite(metrics.percent)) throw new Error('Unsupported metrics schema');
    document.querySelector('#overall').replaceChildren(document.createTextNode(formatPercent(metrics.percent)), Object.assign(document.createElement('small'), { textContent: '%' }));
    document.querySelector('#overall-count').textContent = `${formatNumber(metrics.matched)} / ${formatNumber(metrics.total)}`;
    document.querySelector('#coverage-ring').style.strokeDashoffset = String(320.442 * (1 - Math.min(100, Math.max(0, metrics.percent)) / 100));
    for (const [key, prefix] of [['math', 'Unity.Mathematics'], ['ecs', 'Unity.Entities']]) {
      const group = metrics.groups.find(group => group.name.startsWith(prefix));
      if (!group || !Number.isFinite(group.percent)) continue;
      document.querySelector(`#${key}-percent`).textContent = `${formatPercent(group.percent)}%`;
      document.querySelector(`#${key}-count`).textContent = `${formatNumber(group.matched)} of ${formatNumber(group.total)}`;
      document.querySelector(`#${key}-bar`).style.width = `${Math.min(100, Math.max(0, group.percent))}%`;
    }
  } catch (error) {
    // Keep the explicitly labelled last measured snapshot visible if JSON is unavailable.
    document.querySelector('#overall-count').title = 'Last saved snapshot; the current metrics could not be loaded';
    console.warn('R-Rising metrics unavailable:', error.message);
  }
}
updateMetrics();

const shots = {
  native: { path: 'assets/screenshots/native-original-scene.png', detail: '1280 \u00d7 720 \u00b7 9,474 instances \u00b7 original scene', caption: 'One original scene at its stored positions, native on macOS / Metal. No lighting parity.', alt: 'A mountain scene from V Rising drawn by the native Rust renderer at the entities\u2019 original positions' },
  browser: { path: 'assets/screenshots/browser-original-scene.png', detail: 'Chrome \u00b7 WebGPU \u00b7 314 draw groups', caption: 'The same scene in Chrome through WebAssembly and WebGPU, from a 74.7 MB local export, no page errors.', alt: 'The same V Rising scene drawn in Chrome through WebGPU' }
};
const tabs = [...document.querySelectorAll('[data-shot]')];
function showShot(tab) {
  const shot = shots[tab.dataset.shot];
  for (const item of tabs) {
    const active = item === tab;
    item.classList.toggle('active', active);
    item.setAttribute('aria-selected', String(active));
    item.tabIndex = active ? 0 : -1;
  }
  document.querySelector('#shot-image').src = shot.path;
  document.querySelector('#shot-image').alt = shot.alt;
  document.querySelector('#shot-link').href = shot.path;
  document.querySelector('#shot-detail').textContent = shot.detail;
  document.querySelector('#shot-caption').textContent = shot.caption;
  document.querySelector('#screenshot-stage').setAttribute('aria-labelledby', tab.id);
}
for (const tab of tabs) {
  tab.addEventListener('click', () => showShot(tab));
  tab.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === 'Home' ? tabs[0] : event.key === 'End' ? tabs[tabs.length - 1] : tabs[(tabs.indexOf(tab) + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length];
    next.focus();
    showShot(next);
  });
}

for (const button of document.querySelectorAll('[data-copy]')) {
  button.addEventListener('click', async () => {
    const text = document.getElementById(button.dataset.copy).textContent;
    try {
      await navigator.clipboard.writeText(text);
      button.textContent = 'Copied';
      document.querySelector('#announcement').textContent = 'Command copied';
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(document.getElementById(button.dataset.copy));
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = 'Text selected';
      document.querySelector('#announcement').textContent = 'Command selected for copying';
    }
    setTimeout(() => { button.textContent = 'Copy'; }, 2500);
  });
}
