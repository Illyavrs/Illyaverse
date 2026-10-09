import type { Metadata } from 'next';
import '@fontsource-variable/nunito';
import '@fontsource/noto-sans-tc/400.css';
import '@fontsource/noto-sans-tc/600.css';
import './globals.css';

export const metadata: Metadata = {
  title: 'Kaleidoscope · Illyaverse',
  description: 'Illyaverse 的 stable 與 lazer 啟動器。',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return <html lang="zh-TW"><body>{children}</body></html>;
}
