import Link from 'next/link';
import { getTranslations } from 'next-intl/server';
import { Button } from '@/components/ui/button';
export default async function NotFound() {
  const t = await getTranslations('common');
  return (
    <div className="page-container">
      <div className="empty-state">
        <h1>404</h1>
        <p>{t('notFound')}</p>
        <Button asChild>
          <Link href="/">{t('back')}</Link>
        </Button>
      </div>
    </div>
  );
}
