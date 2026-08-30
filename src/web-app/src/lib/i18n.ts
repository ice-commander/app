// Web-pult localization over the SAME flat-JSON dictionaries as the GTK4 app (merged by
// scripts/gen-i18n.mjs into src/i18n/<lang>.json). Mirrors the Rust `ic-i18n` engine:
//   tr(key)            -> current language, else English, else the key itself
//   trf(key, args)     -> same, with `%{name}` placeholders substituted
// The language is provided by the host via setLang() (e.g. from the app config/state once the
// web version exposes it); until then it defaults to English.

const modules = import.meta.glob<{ default: Record<string, string> }>('../i18n/*.json', {
  eager: true,
});

const dicts: Record<string, Record<string, string>> = {};
for (const path in modules) {
  const code = path.split('/').pop()!.replace('.json', '');
  dicts[code] = modules[path].default;
}

const DEFAULT = 'en';
let current = DEFAULT;

/** Set the active language by code (unknown -> English). */
export function setLang(code: string): void {
  current = dicts[code] ? code : DEFAULT;
}

/** The active language code. */
export function currentLang(): string {
  return current;
}

/** Available language codes (those with a dictionary). */
export function languages(): string[] {
  return Object.keys(dicts);
}

/** Translate a key: current language -> English -> the key itself. */
export function tr(key: string): string {
  return dicts[current]?.[key] ?? dicts[DEFAULT]?.[key] ?? key;
}

/** Translate with `%{name}` substitutions; unknown placeholders are left verbatim. */
export function trf(key: string, args: Record<string, string | number>): string {
  return tr(key).replace(/%\{(\w+)\}/g, (m, name) =>
    Object.prototype.hasOwnProperty.call(args, name) ? String(args[name]) : m,
  );
}
