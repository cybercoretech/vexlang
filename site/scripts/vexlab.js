// VexLab: file tabs, editor source, output panel and a staged CHECK -> LOWER -> RUN.
(function () {
  'use strict';

  // `lower` is what `vexlang ir` / `vexlang target` accept for each program
  // (checked against vexlang 0.1.0-alpha.2).
  const FILES = {
    vexrun: {
      lower: 'lower → vex-scalar-v1',
      label: 'vex run.vex',
      lines: 'let answer = (10 + 2) * 3;\nanswer;',
      check: 'vexlang check vexrun.vex',
      run: 'vexlang run vexrun.vex',
      out: ['vexlang run vexrun.vex', '→ Int(36)'],
    },
    mathvex: {
      lower: 'lower → vex-scalar-v1',
      label: 'math.vex',
      lines: 'let x = 0;\nwhile x < 3 { x = x + 1; }\nx;',
      check: 'vexlang check mathvex.vex',
      run: 'vexlang run mathvex.vex',
      out: ['vexlang run mathvex.vex', '→ Int(3)'],
    },
    pointvex: {
      lower: 'lower → typed IR',
      label: 'point.vex',
      lines: 'record Point { x: i32, y: i32 }\nlet p: Point = Point { x: 2, y: 3 };\np.x + p.y;',
      check: 'vexlang check pointvex.vex',
      run: 'vexlang run pointvex.vex',
      out: ['vexlang run pointvex.vex', '→ Int(5)'],
    },
    appvex: {
      lower: 'interpreter (Result helpers do not lower yet)',
      label: 'app.vex',
      lines: '// vexlang add vexstd\nuse vexstd;\nlet score = clamp(150, 0, 100);\nprintln(score);\ngcd(48, 18);',
      check: 'vexlang check app.vex',
      run: 'vexlang run app.vex',
      // Real output: println prints 100, then the program value.
      printed: ['100'],
      out: ['vexlang run app.vex', '→ Int(6)'],
    },
  };

  const sources = {};
  const outputs = {};
  for (const id of Object.keys(FILES)) {
    const s = document.querySelector(`.vexlab-source[data-file="${id}"]`);
    const o = document.querySelector(`.vexlab-out[data-file="${id}"]`);
    sources[id] = s ? s : null;
    outputs[id] = o ? o : null;
  }

  const statusEl = document.querySelector('.vexlab-status');
  const titleEl = document.querySelector('.vexlab-file');

  function currentId() {
    const on = document.querySelector('.vexlab-tab.is-on');
    return on ? on.getAttribute('data-file') : 'vexrun';
  }

  function showFile(id) {
    for (const key of Object.keys(FILES)) {
      const s = sources[key];
      const o = outputs[key];
      if (s) s.hidden = key !== id;
      if (o) o.hidden = key !== id;
    }
    if (titleEl) titleEl.textContent = FILES[id].label;
    const linesEl = document.querySelector('.vexlab-lines');
    if (linesEl) linesEl.textContent = lineCount(id);
  }

  // f.out[1] is '→ RESULT'; strip the arrow to get the program's value.
  function result(f) {
    return f.out[1].replace(/^→\s*/, '');
  }

  function lineCount(id) {
    return '● ' + FILES[id].lines.split('\n').length + ' lines';
  }

  function renderOutput(f) {
    const printed = (f.printed || [])
      .map(line => '<span class="vexlab-out-line">' + line + '</span>')
      .join('');
    return '<span class="vexlab-out-line ok"><span class="vexlab-out-prompt">$</span> ' + f.run + '</span>' +
      printed +
      '<span class="vexlab-out-line"><span class="vexlab-out-prompt">→</span> ' + result(f) + '</span>';
  }

  function paintStatus(id) {
    const f = FILES[id];
    const el = statusEl;
    if (!el) return;
    el.innerHTML =
      '<span>' + f.check + ' → ok</span>' +
      '<span>0 errors</span>' +
      '<span class="vexlab-lines">' + lineCount(id) + '</span>' +
      '<span>' + f.lower + '</span>' +
      '<span>RUN → ' + f.run + ' → ' + result(f) + '</span>';
  }

  function run() {
    const id = currentId();
    const f = FILES[id];
    const o = outputs[id];
    if (!o) return;
    o.innerHTML = renderOutput(f);
    paintStatus(id);
  }

  document.addEventListener('click', (e) => {
    const tab = e.target.closest('.vexlab-tab');
    if (!tab) return;
    document.querySelectorAll('.vexlab-tab').forEach(t => {
      t.classList.toggle('is-on', t === tab);
      t.setAttribute('aria-selected', t === tab ? 'true' : 'false');
    });
    showFile(tab.getAttribute('data-file'));
  });

  const runBtn = document.getElementById('vexlab-run');
  if (runBtn) runBtn.addEventListener('click', () => {
    const id = currentId();
    const f = FILES[id];
    const o = outputs[id];
    if (o) {
      o.innerHTML = renderOutput(f);
    }
    paintStatus(id);
  });

  // default paint
  for (const key of Object.keys(FILES)) {
    const s = sources[key];
    const o = outputs[key];
    if (s) s.hidden = key !== 'vexrun';
    if (o) o.hidden = key !== 'vexrun';
  }
})();
