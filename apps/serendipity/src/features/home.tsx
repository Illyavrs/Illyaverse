'use client';
import Link from 'next/link';
import { useTranslations } from 'next-intl';
import useSWR from 'swr';
import { ArrowRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useSession } from '@/components/providers';
import { Loading, LoadError, Empty } from '@/components/common';
import { fetcher, hasPermission } from '@/lib/api';
import type { Status, Beatmap, Page } from '@/lib/types';
import { MapRows } from './beatmaps';
export function HomePage() {
  const t = useTranslations('home');
  const c = useTranslations('common');
  const n = useTranslations('nav');
  const m = useTranslations('maps');
  const s = useTranslations('settings');
  const social = useTranslations('social');
  const { user } = useSession();
  const { data: status, isLoading: statusLoading } = useSWR<Status>('/status', fetcher, {
    refreshInterval: 30000,
  });
  const {
    data: maps,
    error,
    isLoading,
    mutate,
  } = useSWR<Page<Beatmap>>('/beatmaps?page_size=3', fetcher);
  return (
    <div className="home-page">
      <section className="hero-cover glass">
        <div className="hero-copy">
          <h1>{t('title')}</h1>
          <p className="hero-subtitle">{t('titleEnd')}</p>
          <p>{t('description')}</p>
          <div className="hero-actions">
            <Button asChild size="lg">
              <Link href="/beatmaps">
                {t('maps')}
                <ArrowRight />
              </Link>
            </Button>
            <Button asChild variant="outline" size="lg">
              <Link href={user ? `/players/${user.id}` : '/register'}>
                {user ? n('profile') : t('join')}
              </Link>
            </Button>
          </div>
        </div>
      </section>
      <div className="community-strip">
        <div className="connection-indicator">
          <span className={status ? 'online-dot' : 'offline-dot'} />
          {statusLoading ? c('loading') : t(status ? 'live' : 'offline')}
        </div>
        {status ? (
          <dl>
            {(
              [
                ['players', status.players_online],
                ['members', status.registered_players],
                ['scores', status.ranked_scores],
                ['beatmaps', status.beatmap_count],
              ] as const
            ).map(([key, value]) => (
              <div key={key}>
                <dd>{value.toLocaleString()}</dd>
                <dt>{t(key)}</dt>
              </div>
            ))}
          </dl>
        ) : !statusLoading ? (
          <p>{t('offlineDescription')}</p>
        ) : null}
      </div>
      <div className="home-body">
        <section className="home-map-section">
          <header className="section-heading">
            <div>
              <h2>{t('explore')}</h2>
              <p>{t('exploreDescription')}</p>
            </div>
            <Link href="/beatmaps" className="text-link">
              {c('viewAll')}
              <ArrowRight />
            </Link>
          </header>
          <div className="glass home-map-panel">
            {isLoading ? (
              <Loading />
            ) : error ? (
              <LoadError retry={() => mutate()} />
            ) : maps?.items.length ? (
              <MapRows items={maps.items} compact />
            ) : (
              <Empty title={t('new')}>
                <p>{t('emptyMapsDescription')}</p>
                <Link href="/beatmaps" className="text-link">
                  {t('maps')}
                  <ArrowRight />
                </Link>
              </Empty>
            )}
          </div>
        </section>
        <aside className="home-connect home-tools glass">
          <h2>{t('tools')}</h2>
          <nav className="home-tool-links" aria-label={t('tools')}>
            <Link href="/rankings">
              {n('rankings')}
              <ArrowRight />
            </Link>
            <Link href="/chat">
              {n('community')}
              <ArrowRight />
            </Link>
            {user && hasPermission(user.permissions, 'beatmaps.upload') && (
              <Link href="/beatmaps/upload">
                {m('upload')}
                <ArrowRight />
              </Link>
            )}
            {user && hasPermission(user.permissions, 'beatmaps.import') && (
              <Link href="/beatmaps/import">
                {m('import')}
                <ArrowRight />
              </Link>
            )}
            {user && (
              <>
                <Link href="/settings?tab=profile">
                  {s('profile')}
                  <ArrowRight />
                </Link>
                <Link href="/settings?tab=account">
                  {s('account')}
                  <ArrowRight />
                </Link>
                <Link href="/settings?tab=social">
                  {social('title')}
                  <ArrowRight />
                </Link>
              </>
            )}
            <Link href="/settings?tab=appearance">
              {n('appearance')}
              <ArrowRight />
            </Link>
          </nav>
          <h3>{t('connect')}</h3>
          <p>{t('notice')}</p>
          <Link href="/connect" className="text-link">
            {t('connect')}
            <ArrowRight />
          </Link>
        </aside>
      </div>
    </div>
  );
}
