'use client';
import { useSearchParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import type { ScoreVariant } from '@/lib/types';

export function useScoreSelection(defaultMode = 0) {
  const search = useSearchParams();
  const rawMode = search.get('mode') ?? String(defaultMode);
  const mode = /^[0-3]$/.test(rawMode) ? rawMode : String(defaultMode);
  const variant: ScoreVariant = mode !== '3' && search.get('variant') === 'relax' ? 'relax' : 'vanilla';
  const selection = new URLSearchParams({ mode, variant });
  function select(nextMode: string, nextVariant: ScoreVariant) {
    const query = new URLSearchParams(search.toString());
    query.set('mode', nextMode);
    query.set('variant', nextMode === '3' ? 'vanilla' : nextVariant);
    query.delete('page');
    window.history.pushState(null, '', `?${query}`);
  }
  return { mode, variant, selection, select, href: (path: string) => `${path}?${selection}` };
}

export function ScoreVariantControl({ mode, value, onChange }: {
  mode: string;
  value: ScoreVariant;
  onChange: (value: ScoreVariant) => void;
}) {
  const t = useTranslations('common');
  return (
    <div className="mode-control" role="group" aria-label={t('scoreVariant')}>
      {(['vanilla', 'relax'] as const).map((variant) => (
        <button key={variant} type="button" aria-pressed={value === variant}
          disabled={mode === '3' && variant === 'relax'}
          title={mode === '3' && variant === 'relax' ? t('relaxUnsupported') : undefined}
          onClick={() => onChange(variant)}>
          {t(variant)}
        </button>
      ))}
    </div>
  );
}
