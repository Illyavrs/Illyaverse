'use client';
import { useTranslations } from 'next-intl';
import { Button } from '@/components/ui/button';
export default function Error({ reset }: { reset: () => void }) {
  const t = useTranslations('common');
  return (
    <div className="page-container">
      <section className="empty-state" role="alert">
        <h1>{t('error')}</h1>
        <Button onClick={reset}>{t('retry')}</Button>
      </section>
    </div>
  );
}
