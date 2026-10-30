// Darkflow site runtime: Cybercore theme chooser, shared footer, project
// signal cards, and the mdBook integration. Shared by the landing page and
// every Field Manual page (loaded there through book.toml additional-js).
//
// Privacy: no analytics. The theme choice is kept in this browser only
// (localStorage "cybercore-theme", the same key the other Cybercore sites use
// on this origin). Repository and CI numbers are read from GitHub's public
// API by the visitor's browser.
(() => {
  'use strict';

  const REPO = 'cybercoretech/vexlang';
  const STORE = 'cybercore-theme';
  const THEMES = window.VEXLANG_THEME_DATA || [];
  const FAMILY_LABELS = {
    cyberdyne: 'CYBERDYNE', cyberpunk: 'CYBERPUNK', default: 'DEFAULT / CLASSICS', dystopian: 'DYSTOPIAN',
    neosynth: 'NEOSYNTH', 'omarchy-live': 'OMARCHY LIVE', synthwave: 'SYNTHWAVE', DWCode: 'DWCODE',
  };
  const root = document.documentElement;
  const $ = (sel, el = document) => el.querySelector(sel);
  const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];
  const el = (tag, attrs = {}, ...kids) => {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
      if (k === 'class') node.className = v;
      else if (k === 'text') node.textContent = v;
      else node.setAttribute(k, v);
    }
    for (const kid of kids) node.append(kid);
    return node;
  };
  const store = {
    get() { try { return localStorage.getItem(STORE); } catch { return null; } },
    set(v) { try { localStorage.setItem(STORE, v); } catch { /* private mode */ } },
  };

  // ── theme ─────────────────────────────────────────────────────────────────
  const byId = new Map(THEMES.map(t => [t.id, t]));
  const shortName = t => {
    const fam = (FAMILY_LABELS[t.family] || t.family).split(' ')[0].toLowerCase();
    const words = t.name.split(' ');
    return (words[0].toLowerCase() === fam && words.length > 1 ? words.slice(1) : words).join(' ').toUpperCase();
  };

  function applyTheme(id, { persist = false } = {}) {
    const t = byId.get(id) || byId.get(root.dataset.theme) || THEMES[0];
    if (!t) return;
    const vars = {
      '--theme-bg': t.surfaces.window,
      '--theme-panel': t.surfaces.shell,
      '--theme-line': t.surfaces.border,
      '--theme-text': t.text.primary,
      '--theme-muted': t.text.muted,
      '--theme-accent': t.accents.primary,
      '--theme-secondary': t.accents.secondary,
      '--theme-success': t.accents.success,
      '--theme-warning': t.accents.warning,
    };
    for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
    root.dataset.theme = t.id;
    root.style.colorScheme = t.appearance === 'light' ? 'light' : 'dark';
    const meta = $('meta[name="theme-color"]');
    if (meta) meta.content = t.surfaces.window;
    const label = $('#theme-current');
    if (label) label.textContent = shortName(t);
    $$('.theme-option').forEach(b => b.classList.toggle('selected', b.dataset.themeId === t.id));
    $$('.theme-option').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.themeId === t.id)));
    if (persist) store.set(t.id);
  }

  function buildThemeOptions() {
    const box = $('#theme-options');
    if (!box) return;
    const families = [...new Set(THEMES.map(t => t.family))];
    box.textContent = '';
    for (const fam of families) {
      box.append(el('div', { class: 'theme-family', 'data-family': fam, text: FAMILY_LABELS[fam] || fam.toUpperCase() }));
      for (const t of THEMES.filter(x => x.family === fam)) {
        const sw = el('span', { class: 'theme-swatches', 'aria-hidden': 'true' });
        for (const c of [t.accents.primary, t.accents.secondary, t.accents.success, t.surfaces.window]) {
          const dot = el('i'); dot.style.background = c; sw.append(dot);
        }
        const b = el('button', { type: 'button', class: 'theme-option', 'data-theme-id': t.id, 'data-family': fam, 'aria-pressed': 'false' },
          el('span', { text: t.name }), sw);
        b.addEventListener('click', () => applyTheme(t.id, { persist: true }));
        box.append(b);
      }
    }
    const note = $('#theme-note') || $('.theme-note');
    if (note) note.textContent = `${THEMES.length} palettes · ${families.length} families · saved in this browser only`;
    const search = $('#theme-search');
    search?.addEventListener('input', () => {
      const q = search.value.trim().toLowerCase();
      const visibleFamilies = new Set();
      $$('.theme-option', box).forEach(b => {
        const show = !q || b.textContent.toLowerCase().includes(q) || b.dataset.family.toLowerCase().includes(q);
        b.hidden = !show;
        if (show) visibleFamilies.add(b.dataset.family);
      });
      $$('.theme-family', box).forEach(h => { h.hidden = !visibleFamilies.has(h.dataset.family); });
    });
  }

  function headerMarkup() {
    const isKw = location.pathname.endsWith('keywords.html');
    const docs = isKw ? './docs/index.html' : 'docs/index.html';
    const sourceLink = `<a href="https://github.com/${REPO}" target="_blank" rel="noopener noreferrer">SOURCE ↗</a>`;
    const navItems = isKw
      ? `<a href="./">home</a><a href="./keywords.html">keywords</a><a href="${docs}">docs</a>${sourceLink}`
      : `<a href="#language">language</a><a href="#milestones-section">milestones</a><a href="#playground">playground</a><a href="keywords.html">keywords</a><a href="docs/index.html">docs</a>${sourceLink}`;
    return `
      <a class="brand-lockup" href="./" aria-label="VexLang home">
        <span class="brand-mark">V</span><span class="brand-name">EXLANG</span>
      </a>
      <nav class="main-nav" aria-label="Primary navigation">${navItems}</nav>
      <button class="theme-trigger" type="button" data-theme-open aria-haspopup="dialog"><span class="theme-dot"></span><span>CYBERCORE THEME</span><b id="theme-current">—</b><span aria-hidden="true">⌄</span></button>
    `;
  }

  function dialogsMarkup() {
    return `
      <dialog class="theme-dialog" id="theme-dialog" aria-labelledby="theme-title">
        <div class="dialog-top"><span>CYBERCORE / PALETTE SELECTOR</span><button type="button" data-dialog-close aria-label="Close">×</button></div>
        <div class="dialog-body">
          <p class="eyebrow">COLOR SYSTEM</p>
          <h2 id="theme-title">Tune the signal.</h2>
          <p>Choose from the canonical Cybercore palette collection. Your selection is saved only in this browser.</p>
          <label class="theme-search-label" for="theme-search">FILTER THEMES</label>
          <input id="theme-search" type="search" placeholder="Search palettes…">
          <div class="theme-options" id="theme-options"></div>
          <small class="theme-note" id="theme-note"></small>
        </div>
      </dialog>
    `;
  }

  // Init
  const header = document.querySelector('header.shell.header');
  if (header) header.innerHTML = headerMarkup();
  if (!document.querySelector('#theme-dialog')) document.body.insertAdjacentHTML('beforeend', dialogsMarkup());
  buildThemeOptions();
  applyTheme(store.get() || null, { persist: false });

  // ── dialogs and navigation ────────────────────────────────────────────────
  function wireDialogs() {
    $$('[data-theme-open]').forEach(b => b.addEventListener('click', () => {
      const d = $('#theme-dialog');
      d?.showModal();
      $('.theme-option.selected', d)?.scrollIntoView({ block: 'nearest' });
    }));
    $$('dialog').forEach(d => {
      d.addEventListener('click', e => { if (e.target === d) d.close(); });
      $$('[data-dialog-close]', d).forEach(b => b.addEventListener('click', () => d.close()));
    });
  }

  wireDialogs();
})();
