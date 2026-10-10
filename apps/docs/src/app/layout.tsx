import { RootProvider } from 'fumadocs-ui/provider/next';
import type { Metadata } from 'next';
import './global.css';

export const metadata: Metadata = {
  title: { default: 'Illyaverse 部署文件', template: '%s · Illyaverse 部署文件' },
  description: '部署、設定與維運 Illyaverse 社群伺服器的管理員文件。',
};

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <html lang="zh-TW" suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <RootProvider search={{ options: { api: '/api/search' } }}>{children}</RootProvider>
      </body>
    </html>
  );
}
