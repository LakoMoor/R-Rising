'use strict';
const formatNumber = number => new Intl.NumberFormat('ru-RU').format(number);
const formatPercent = number => new Intl.NumberFormat('ru-RU', { maximumFractionDigits: 1, minimumFractionDigits: 1 }).format(number);
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
      document.querySelector(`#${key}-count`).textContent = `${formatNumber(group.matched)} из ${formatNumber(group.total)}`;
      document.querySelector(`#${key}-bar`).style.width = `${Math.min(100, Math.max(0, group.percent))}%`;
    }
  } catch (error) {
    // Keep the explicitly labelled last measured snapshot visible if JSON is unavailable.
    document.querySelector('#overall-count').title = 'Последний сохранённый срез; текущие метрики не удалось загрузить';
    console.warn('R-Rising metrics unavailable:', error.message);
  }
}
updateMetrics();

const shots = {
  native: { path: 'assets/screenshots/native-content.png', detail: '1600 × 900 · 48 объектов · 26 текстур', caption: 'Нативный Rust-рендерер на macOS / Metal.', alt: 'Оригинальные модели V Rising в нативном Rust-рендерере, размещённые демонстрационной сеткой' },
  browser: { path: 'assets/screenshots/browser-webgpu.png', detail: 'Chrome · WebGPU · 48 объектов', caption: 'Тот же Rust-рендерер в Chrome через WebAssembly / WebGPU.', alt: 'Браузерная страница R-Rising с оригинальными моделями, отрисованными через Rust и WebGPU' }
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
      button.textContent = 'Скопировано';
      document.querySelector('#announcement').textContent = 'Команда скопирована';
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(document.getElementById(button.dataset.copy));
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = 'Текст выделен';
      document.querySelector('#announcement').textContent = 'Команда выделена для копирования';
    }
    setTimeout(() => { button.textContent = 'Копировать'; }, 2500);
  });
}
