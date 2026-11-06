// VexLang site runtime: universal Cybercore theme chooser, shared header,
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
  const root = document.documentElement;

  // Themes: the universal Cybercore chooser (600 registry themes + 12 shared
  // signature mashups). Palette slots map onto the --theme-* variables only.
  window.SITE = Object.assign(window.SITE || {}, { defaultTheme: 'neon-night', themeVars: { bg: '--theme-bg', panel: '--theme-panel', line: '--theme-line', white: '--theme-text', muted: '--theme-muted', cyan: '--theme-accent', purple: '--theme-secondary', acid_green: '--theme-success', orange: '--theme-warning', hot_pink: null, red: null } });
  const CHOOSER_KIT = 'https://cybercoretech.github.io/kit/';
  if (!document.querySelector('link[data-ccx-css]')) {
    const css = document.createElement('link');
    css.rel = 'stylesheet'; css.href = `${CHOOSER_KIT}chooser.css`; css.dataset.ccxCss = '';
    document.head.append(css);
  }
  import(`${CHOOSER_KIT}chooser.js`).catch(e => console.warn('Cybercore theme chooser unavailable', e));
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
      <cybercore-theme-chooser adopt><button class="theme-trigger" type="button">Neon Night</button></cybercore-theme-chooser>
    `;
  }

  // Init
  const header = document.querySelector('header.shell.header');
  if (header) header.innerHTML = headerMarkup();

  // ── dialogs and navigation ────────────────────────────────────────────────
  function wireDialogs() {
    $$('dialog').forEach(d => {
      d.addEventListener('click', e => { if (e.target === d) d.close(); });
      $$('[data-dialog-close]', d).forEach(b => b.addEventListener('click', () => d.close()));
    });
  }

  wireDialogs();
})();
