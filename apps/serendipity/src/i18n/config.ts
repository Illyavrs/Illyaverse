export const locales = ['zh-TW', 'en', 'ja'] as const;
export type Locale = (typeof locales)[number];
export function isLocale(value: unknown): value is Locale {
  return locales.includes(value as Locale);
}
