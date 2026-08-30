// Merge the shared flat-JSON dictionaries (app + all UI components) into one dictionary per
// language for the web pult, so the web version reads the SAME localization as GTK4. Run from
// the repo root (npm prebuild hook). Output: src/web-app/src/i18n/<lang>.json.
import { readFileSync, writeFileSync, mkdirSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const LANGS = [
  'en', 'ru', 'pl', 'cs', 'sk', 'de', 'es', 'uk', 'it', 'fr', 'ro', 'hu', 'be', 'bg', 'sr',
];

const here = dirname(fileURLToPath(import.meta.url));      // src/web-app/scripts
const repoRoot = resolve(here, '..', '..', '..');          // repo root
const outDir = resolve(here, '..', 'src', 'i18n');         // src/web-app/src/i18n

// Source locale dirs: the app, the web-pult's own strings, and every UI component.
const sources = [
  join(repoRoot, 'src', 'gtk-app', 'locales'),
  join(repoRoot, 'src', 'web-app', 'locales'),
];
const uiCommon = join(repoRoot, 'src', 'ui-common');
if (!existsSync(uiCommon)) {
  console.error(`i18n: component locales not found at ${uiCommon}`);
  process.exit(1);
}
for (const crate of readdirSync(uiCommon)) {
  const dir = join(uiCommon, crate, 'locales');
  if (existsSync(dir)) sources.push(dir);
}

mkdirSync(outDir, { recursive: true });
let totalKeys = 0;
for (const lang of LANGS) {
  const merged = {};
  for (const dir of sources) {
    const file = join(dir, `${lang}.json`);
    if (!existsSync(file)) continue;
    Object.assign(merged, JSON.parse(readFileSync(file, 'utf8')));
  }
  const sorted = Object.fromEntries(Object.keys(merged).sort().map((k) => [k, merged[k]]));
  writeFileSync(join(outDir, `${lang}.json`), JSON.stringify(sorted, null, 2) + '\n');
  if (lang === 'en') totalKeys = Object.keys(sorted).length;
}
console.log(`i18n: wrote ${LANGS.length} merged dictionaries (${totalKeys} en keys) to ${outDir}`);
