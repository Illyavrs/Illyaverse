import type { NextConfig } from 'next';
import createNextIntlPlugin from 'next-intl/plugin';
import path from 'node:path';
const withNextIntl = createNextIntlPlugin('./src/i18n/request.ts');
const config: NextConfig = {
  output: 'standalone',
  outputFileTracingRoot: path.resolve(process.cwd(), '../..'),
  async rewrites() {
    const origin = process.env.API_INTERNAL_URL || 'http://127.0.0.1:8080';
    return [
      { source: '/api/v1/:path*', destination: `${origin}/api/v1/:path*` },
      { source: '/media/:path*', destination: `${origin}/media/:path*` },
    ];
  },
  async headers() {
    return [
      {
        source: '/:path*',
        headers: [
          { key: 'X-Content-Type-Options', value: 'nosniff' },
          { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
          { key: 'X-Frame-Options', value: 'DENY' },
        ],
      },
    ];
  },
};
export default withNextIntl(config);
