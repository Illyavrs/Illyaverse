import type { Metadata } from 'next';
import { NextIntlClientProvider } from 'next-intl';
import { getLocale } from 'next-intl/server';
import '@fontsource-variable/nunito';
import '@fontsource/noto-sans-tc/400.css';
import '@fontsource/noto-sans-tc/600.css';
import './globals.css';
import '@/features/home.css';
import '@/features/profile.css';
import '@/styles/rank-history.css';
import '@/features/score-presentation.css';
import '@/features/announcements.css';
import '@/styles/map-discovery.css';
import '@/styles/controls.css';
import { Providers } from '@/components/providers';
import { Shell } from '@/components/shell';
export const metadata: Metadata = {
  title: { default: 'Illyaverse · osu! community server', template: '%s · Illyaverse' },
  description:
    'An independent osu! community server with beatmaps, rankings, player profiles and chat.',
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
