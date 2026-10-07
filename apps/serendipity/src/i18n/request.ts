import { cookies } from 'next/headers';
import { getRequestConfig } from 'next-intl/server';
import { isLocale } from './config';
export default getRequestConfig(async () => {
  const jar = await cookies();
  let value = jar.get('illya_locale')?.value;
  const session = jar.get('illya_session')?.value;
  if (session) {
    try {
      const response = await fetch(
        `${process.env.API_INTERNAL_URL || 'http://127.0.0.1:8080'}/api/v1/me/preferences`,
        {
          headers: { cookie: `illya_session=${session}` },
          cache: 'no-store',
          signal: AbortSignal.timeout(2000),
        },
      );
      if (response.ok) {
        const preferences = await response.json();
        if (isLocale(preferences.locale)) value = preferences.locale;
      }
    } catch {
      /* The browser preference remains usable if Sanctuary is unavailable. */
    }
  }
  const locale = isLocale(value) ? value : 'zh-TW';
  return { locale, messages: (await import(`../../messages/${locale}.json`)).default };
});
