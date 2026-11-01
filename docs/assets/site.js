// Theme toggle and copy buttons. The sheet menu, parallax, diagram and text rotation come from defuss-shadcn's all.min.js.
// VERIFIED: its 0.9.11 bundle references neither #theme-toggle nor .mk-code-block-copy (2026-11-01), so this page wires
// them. VERIFIED: in Playwright a code block's innerText keeps one line per <span>, so a copy runs as pasted; with tabs,
// the visible <pre> is the selected one (the component shows it through :has(:checked)).
const $ = id => document.getElementById(id);

const theme = $('theme-toggle');
const setTheme = dark => {
  document.documentElement.classList.toggle('dark', dark);
  document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
  theme.checked = dark;
  try { localStorage.setItem('defuss-shadcn-theme', JSON.stringify(dark ? 'dark' : 'light')); } catch {}
};
theme.checked = document.documentElement.classList.contains('dark');
theme.addEventListener('change', () => setTheme(theme.checked));

const copyText = async (text, status, label) => {
  try {
    await navigator.clipboard.writeText(text);
    status.textContent = 'Copied';
  } catch {
    status.textContent = 'Copy failed';
  }
  if (label) label.textContent = status.textContent;
  setTimeout(() => {
    status.textContent = '';
    if (label) label.textContent = 'Copy prompt';
  }, 2000);
};
for (const button of document.querySelectorAll('.mk-code-block-copy')) {
  const figure = button.closest('.mk-code-block');
  button.addEventListener('click', () => copyText(
    [...figure.querySelectorAll('pre')].find(pre => pre.checkVisibility()).innerText.trim(),
    figure.querySelector('.mk-code-block-status')));
}
$('copy-prompt').addEventListener('click', () => copyText(
  [...$('agent-prompt').querySelectorAll('pre:not([data-cursor]) code')].map(code => code.textContent).join('\n\n'),
  $('copy-prompt-status'), $('copy-prompt').querySelector('span')));
