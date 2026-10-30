// Opens the example dialogs on the landing page. Every trigger carries a
// data-dialog attribute pointing at the id of its <dialog>. The theme runtime
// (theme/cybercore.js) already wires close buttons and backdrop clicks for all
// dialogs; the close wiring below only matters if that script fails to load.
document.querySelectorAll('[data-dialog]').forEach(trigger => {
  const dialog = document.getElementById(trigger.dataset.dialog);
  if (!dialog) return;
  trigger.addEventListener('click', () => dialog.showModal());
  dialog.querySelectorAll('[data-dialog-close]').forEach(button => {
    button.addEventListener('click', () => dialog.close());
  });
  dialog.addEventListener('click', event => {
    if (event.target === dialog) dialog.close();
  });
});
