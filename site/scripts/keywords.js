// VexLang keyword index interactions: search, status filter, category chips,
// result count, copy-to-clipboard for the selected keyword.
(function () {
  'use strict';

  const state = {
    query: '',
    status: 'all',
    category: 'all',
  };

  const els = {
    search: document.getElementById('kw-search'),
    status: document.querySelector('.status-filter'),
    chips: document.getElementById('kw-chips'),
    count: document.getElementById('kw-count'),
    reset: document.getElementById('kw-reset'),
    empty: document.getElementById('kw-empty'),
    emptyReset: document.getElementById('kw-empty-reset'),
    list: document.getElementById('kw-list'),
  };

  function apply() {
    const q = state.query.trim().toLowerCase();
    const entries = window.VEXLANG_KEYWORD_INDEX || [];
    const visible = entries.filter(e => {
      if (state.status !== 'all' && e.s !== state.status) return false;
      if (state.category !== 'all' && String(e.c) !== String(state.category)) return false;
      if (!q) return true;
      const hay = [e.k, e.name, e.s, e.slug, JSON.stringify(e)].join(' ').toLowerCase();
      // the code snippets are rendered into the DOM, so the card text is enough
      const card = document.getElementById(e.slug);
      const txt = card ? (card.textContent || '').toLowerCase() : hay;
      return txt.includes(q) || hay.includes(q);
    });

    document.querySelectorAll('article.kw').forEach(card => {
      const show = visible.some(v => v.slug === card.id);
      card.hidden = !show;
    });
    document.querySelectorAll('section.cat').forEach(sec => {
      const on = state.category === 'all' || String(state.category) === sec.dataset.category;
      sec.hidden = !on;
    });

    if (els.count) els.count.textContent = String(visible.length);
    if (els.empty) els.empty.hidden = visible.length > 0;
    if (els.reset) els.reset.hidden = visible.length === (window.VEXLANG_KEYWORD_INDEX || []).length && !state.query && state.status === 'all' && state.category === 'all';
    if (els.list) els.list.setAttribute('aria-hidden', String(visible.length === 0));
  }

  function bindStatus() {
    if (!els.status) return;
    els.status.querySelectorAll('[data-status]').forEach(btn => {
      btn.addEventListener('click', () => {
        state.status = btn.getAttribute('data-status');
        document.querySelectorAll('.status-filter .seg').forEach(b => b.classList.toggle('is-on', b === btn));
        apply();
      });
    });
  }

  function bindChips() {
    if (!els.chips) return;
    els.chips.addEventListener('click', e => {
      const chip = e.target.closest('[data-cat]');
      if (!chip) return;
      state.category = chip.getAttribute('data-cat');
      els.chips.querySelectorAll('.chip').forEach(b => b.classList.toggle('is-on', b === chip));
      apply();
    });
  }

  function bindSearch() {
    if (!els.search) return;
    els.search.addEventListener('input', e => {
      state.query = e.target.value;
      apply();
    });
  }

  function bindReset() {
    if (els.reset) {
      els.reset.addEventListener('click', () => {
        state.query = '';
        state.status = 'all';
        state.category = 'all';
        if (els.search) els.search.value = '';
        document.querySelectorAll('.status-filter .seg').forEach(b => b.classList.toggle('is-on', b.getAttribute('data-status') === 'all'));
        document.querySelectorAll('.chip').forEach(b => b.classList.toggle('is-on', b.getAttribute('data-cat') === 'all'));
        apply();
      });
    }
    if (els.emptyReset) {
      els.emptyReset.addEventListener('click', () => {
        state.query = '';
        state.status = 'all';
        state.category = 'all';
        if (els.search) els.search.value = '';
        document.querySelectorAll('.status-filter .seg').forEach(b => b.classList.toggle('is-on', b.getAttribute('data-status') === 'all'));
        document.querySelectorAll('.chip').forEach(b => b.classList.toggle('is-on', b.getAttribute('data-cat') === 'all'));
        apply();
      });
    }
  }

  function bindCopy() {
    document.addEventListener('click', e => {
      const kw = e.target.closest('[data-keyword]');
      if (!kw) return;
      const text = kw.getAttribute('data-keyword');
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).catch(() => {});
      }
    });
  }

  document.addEventListener('DOMContentLoaded', () => {
    bindStatus();
    bindChips();
    bindSearch();
    bindReset();
    bindCopy();
    document.addEventListener('keydown', e => {
      if (e.key === '/' && document.activeElement.tagName !== 'INPUT' && document.activeElement.tagName !== 'TEXTAREA') {
        e.preventDefault();
        els.search && els.search.focus();
      }
    });
    apply();
  });
})();
