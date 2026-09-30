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

// What the host served at startup. It carries the language the app is actually
// set to and the keys plugins registered at runtime, neither of which can be
// known at build time, so it is consulted first.
let served: Record<string, string> = {};

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

/** Adopt the host's language and dictionary (see `GET /api/i18n`). */
export function adopt(lang: string, keys: Record<string, string>): void {
  setLang(lang);
  served = keys;
}

/** Translate a key: host -> current language -> English -> the key itself. */
export function tr(key: string): string {
  return served[key] ?? dicts[current]?.[key] ?? dicts[DEFAULT]?.[key] ?? key;
}

/** The same lookup, but absent rather than the key, for a plugin's optional text. */
export function trOptional(key: string): string | undefined {
  const found = served[key] ?? dicts[current]?.[key] ?? dicts[DEFAULT]?.[key];
  return found === undefined || found === key ? undefined : found;
}

/** Translate with `%{name}` substitutions; unknown placeholders are left verbatim. */
export function trf(key: string, args: Record<string, string | number>): string {
  return tr(key).replace(/%\{(\w+)\}/g, (m, name) =>
    Object.prototype.hasOwnProperty.call(args, name) ? String(args[name]) : m,
  );
}
