type OS = 'mac' | 'win' | 'linux';

const widget = document.querySelector<HTMLElement>('[data-os-widget]');

if (widget) {
  const tabs = widget.querySelectorAll<HTMLButtonElement>('[role="tab"]');
  const panels = widget.querySelectorAll<HTMLElement>('[role="tabpanel"]');

  const detect = (): OS => {
    const ua = navigator.userAgent;
    if (/Win/i.test(ua)) return 'win';
    if (/Linux|X11/i.test(ua) && !/Android/i.test(ua)) return 'linux';
    return 'mac';
  };

  const activate = (os: string) => {
    tabs.forEach((b) => {
      const on = b.dataset.os === os;
      b.setAttribute('aria-selected', String(on));
      b.tabIndex = on ? 0 : -1;
      b.classList.toggle('is-active', on);
    });
    panels.forEach((p) => {
      p.hidden = p.dataset.panel !== os;
    });
  };

  activate(detect());

  tabs.forEach((b) => {
    b.addEventListener('click', () => {
      const os = b.dataset.os;
      if (os) activate(os);
    });
    b.addEventListener('keydown', (e) => {
      const arr = Array.from(tabs);
      const i = arr.indexOf(b);
      if (e.key === 'ArrowRight') {
        e.preventDefault();
        const next = arr[(i + 1) % arr.length];
        if (next) { next.focus(); next.click(); }
      }
      if (e.key === 'ArrowLeft') {
        e.preventDefault();
        const prev = arr[(i - 1 + arr.length) % arr.length];
        if (prev) { prev.focus(); prev.click(); }
      }
    });
  });

  widget.querySelectorAll<HTMLElement>('[data-panel]').forEach((panel) => {
    const btn = panel.querySelector<HTMLButtonElement>('[data-copy]');
    const cmd = panel.querySelector<HTMLElement>('[data-cmd]');
    const def = panel.querySelector<HTMLElement>('[data-copy-default]');
    const done = panel.querySelector<HTMLElement>('[data-copy-done]');
    if (!btn || !cmd || !def || !done) return;
    btn.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(cmd.textContent || '');
        def.hidden = true;
        done.hidden = false;
        window.setTimeout(() => {
          def.hidden = false;
          done.hidden = true;
        }, 1400);
      } catch {}
    });
  });
}
