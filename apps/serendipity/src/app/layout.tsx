import type { Metadata } from 'next';
import { NextIntlClientProvider } from 'next-intl';
import { getLocale } from 'next-intl/server';
import '@fontsource-variable/nunito';
import '@fontsource/noto-sans-tc/400.css';
import '@fontsource/noto-sans-tc/600.css';
import './globals.css';
import { Providers } from '@/components/providers';
import { Shell } from '@/components/shell';
export const metadata: Metadata = {
  title: { default: 'Illyaverse — find your rhythm', template: '%s · Illyaverse' },
  description: 'An independent osu! rhythm community. Your songs, your personal bests, your world.',
};
export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const locale = await getLocale();
  return (
    <html lang={locale} suppressHydrationWarning>
      <body>
        <NextIntlClientProvider>
          <Providers>
            <Shell>{children}</Shell>
          </Providers>
        </NextIntlClientProvider>
      </body>
    </html>
  );
}
