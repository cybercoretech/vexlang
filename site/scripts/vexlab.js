// VexLab: file tabs, editor source, output panel and a staged CHECK -> LOWER -> RUN.
(function () {
  'use strict';

  const FILES = {
    vexrun: {
      label: 'vex run.vex',
      lines: 'let answer = (10 + 2) * 3;\nanswer;',
      check: 'vexlang check vexrun.vex',
      run: 'vexlang run vexrun.vex',
      out: ['vexlang run vexrun.vex', '→ Int(36)'],
    },
    mathvex: {
      label: 'math.vex',
      lines: 'let x = 0;\nwhile x < 3 { x = x + 1; }\nx;',
      check: 'vexlang check mathvex.vex',
      run: 'vexlang run mathvex.vex',
      out: ['vexlang run mathvex.vex', '→ Int(3)'],
    },
    pointvex: {
      label: 'point.vex',
      lines: 'record Point { x: i32, y: i32 }\nlet p: Point = Point { x: 2, y: 3 };\np.x + p.y;',
      check: 'vexlang check pointvex.vex',
      run: 'vexlang run pointvex.vex',
      out: ['vexlang run pointvex.vex', '→ Int(5)'],
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
  }

  function paintStatus(id) {
    const f = FILES[id];
    const el = statusEl;
    if (!el) return;
    el.innerHTML =
      '<span>' + f.check + ' → ok</span>' +
      '<span>0 errors</span>' +
      '<span>lower → vex-scalar-v1</span>' +
      '<span>RUN → ' + f.run + ' → ' + (f.out[1].split(' → ')[1] || 'Int(0)') + '</span>';
  }

  function run() {
    const id = currentId();
    const f = FILES[id];
    const o = outputs[id];
    if (!o) return;
    o.innerHTML = '<div class="vexlab-out-line ok"><span class="vexlab-out-prompt">$</span> ' + f.run + '</div><div class="vexlab-out-line"><span class="vexlab-out-prompt">→</span>' + (f.out[1].split(' → ')[1] || 'Int(0)') + '</div>';
    paintStatus(id);
  }

  document.addEventListener('click', (e) => {
    const tab = e.target.closest('.vexlab-tab');
    if (!tab) return;
    document.querySelectorAll('.vexlab-tab').forEach(t => t.classList.toggle('is-on', t === tab));
    showFile(tab.getAttribute('data-file'));
  });

  const runBtn = document.getElementById('vexlab-run');
  if (runBtn) runBtn.addEventListener('click', () => {
    const id = currentId();
    const f = FILES[id];
    const o = outputs[id];
    if (o) {
      o.innerHTML = '<div class="vexlab-out-line ok"><span class="vexlab-out-prompt">$</span> ' + f.run + '</div><div class="vexlab-out-line"><span class="vexlab-out-prompt">→</span>' + (f.out[1].split(' → ')[1] || 'Int(0)') + '</div>';
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
