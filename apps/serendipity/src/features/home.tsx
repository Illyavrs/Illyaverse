'use client';
import Link from 'next/link';
import { useTranslations } from 'next-intl';
import useSWR from 'swr';
import { ArrowRight, ArrowUpRight, Headphones, Palette, MessageCircle, Disc3 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useSession } from '@/components/providers';
import { Loading, LoadError, Empty } from '@/components/common';
import { fetcher } from '@/lib/api';
import type { Status, Beatmap, Page } from '@/lib/types';
import { MapRows } from './beatmaps';
export function HomePage() {
  const t = useTranslations('home');
  const c = useTranslations('common');
  const { user } = useSession();
  const { data: status } = useSWR<Status>('/status', fetcher, { refreshInterval: 30000 });
  const {
    data: maps,
    error,
    isLoading,
    mutate,
  } = useSWR<Page<Beatmap>>('/beatmaps?page_size=3', fetcher);
  return (
    <div className="home-page">
      <section className="hero-cover">
        <div className="hero-art" aria-hidden="true" />
        <div className="hero-shade" aria-hidden="true" />
        <div className="hero-copy">
          <h1>
            {t('title')}
            <br />
            <span>{t('titleEnd')}</span>
          </h1>
          <p>{t('description')}</p>
          <div className="hero-actions">
            <Button asChild size="lg">
              <Link href={user ? '/beatmaps' : '/register'}>
                {t(user ? 'maps' : 'join')}
                <ArrowRight />
              </Link>
            </Button>
            <Button asChild variant="outline" size="lg">
              <Link href="/connect">
                <Headphones />
                {t('connect')}
              </Link>
            </Button>
          </div>
        </div>
        <div className="client-rail">
          <span>
            <Disc3 />
            {t('stable')}
          </span>
          <span>
            <Disc3 />
            {t('lazer')}
          </span>
          <Link href="/connect" aria-label={t('connect')}>
            <ArrowUpRight />
          </Link>
        </div>
      </section>
      <div className="community-strip">
        <div className="connection-indicator">
          <span className={status ? 'online-dot' : 'offline-dot'} />
          {t(status ? 'live' : 'offline')}
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
        ) : (
          <p>{t('offlineDescription')}</p>
        )}
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
                <Link href="/beatmaps" className="text-link">
                  {t('maps')}
                  <ArrowRight />
                </Link>
              </Empty>
            )}
          </div>
        </section>
        <aside className="home-connect glass">
          <Headphones className="feature-icon" />
          <h2>{t('connect')}</h2>
          <p>{t('notice')}</p>
          <Link href="/connect" className="text-link">
            {t('connect')}
            <ArrowRight />
          </Link>
        </aside>
        <section className="community-banner">
          <div className="community-symbol" aria-hidden="true">
            <MessageCircle />
          </div>
          <div>
            <h2>{t('community')}</h2>
            <p>{t('communityDescription')}</p>
          </div>
          <Button asChild variant="outline">
            <Link href="/chat">
              {t('chat')}
              <ArrowRight />
            </Link>
          </Button>
        </section>
        <section className="personalize-banner glass">
          <div>
            <Palette />
            <h2>{t('personalize')}</h2>
            <p>{t('personalizeDescription')}</p>
          </div>
          <Button asChild>
            <Link href="/settings">
              {t('appearance')}
              <ArrowUpRight />
            </Link>
          </Button>
        </section>
      </div>
    </div>
  );
}
