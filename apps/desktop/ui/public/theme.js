// Runs before first paint (a blocking script in <head>) so a dark window never flashes white. src/theme.ts keeps
// the same rule in sync with the saved setting afterwards.
;(function () {
  var t = 'system'
  try { t = localStorage.getItem('typeshelf.theme') || t } catch {}
  if (location.pathname.indexOf('/print') === 0) t = 'light'
  else if (t !== 'light' && t !== 'dark') t = matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  document.documentElement.setAttribute('data-theme', t)
})()
