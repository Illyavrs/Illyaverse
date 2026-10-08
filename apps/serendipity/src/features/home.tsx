'use client';
import Link from 'next/link';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR from 'swr';
import { ArrowRight, Download, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useSession } from '@/components/providers';
import { Avatar, Loading, LoadError } from '@/components/common';
import { fetcher, hasPermission } from '@/lib/api';
import { scoreMods } from '@/lib/score-display';
import type { Status, Beatmap, Leader, Score, Page } from '@/lib/types';
import { MapRows } from './beatmaps';
import { AnnouncementsPreview } from './announcements';

const modes = ['osu!', 'taiko', 'catch', 'mania'];

export function HomePage() {
  const t = useTranslations('home');
  const c = useTranslations('common');
  const n = useTranslations('nav');
  const m = useTranslations('maps');
  const s = useTranslations('settings');
  const social = useTranslations('social');
  const f = useFormatter();
  const { user } = useSession();
  const status = useSWR<Status>('/status', fetcher, { refreshInterval: 30000 });
  const maps = useSWR<Page<Beatmap>>('/beatmaps?page_size=3', fetcher);
  const leaders = useSWR<Page<Leader>>('/leaderboard?mode=0&variant=vanilla&page_size=5', fetcher);
  // Exclude private pending/rejected submissions even for owners and moderators.
  const scores = useSWR<Page<Score>>('/scores?verification=verified&page_size=4', fetcher);
  const publicScores =
    scores.data?.items.filter((score) => score.verification === 'verified') ?? [];
  const profileHref = user ? `/players/${user.id}` : '/register';
  const capabilities = [
    { title: 'playTitle', description: 'playDescription', href: '/rankings' },
    { title: 'mapsTitle', description: 'mapsDescription', href: '/beatmaps' },
    { title: 'profileTitle', description: 'profileDescription', href: '/settings?tab=appearance' },
    { title: 'communityTitle', description: 'communityFeatures', href: '/chat' },
  ] as const;

  return (
    <div className="home-page home-community">
      <section className="home-introduction glass" aria-labelledby="home-title">
        <div className="home-intro-copy">
          <h1 id="home-title">{t('title')}</h1>
          <p className="home-subtitle">{t('titleEnd')}</p>
          <p className="home-description">{t('description')}</p>
          <div className="home-primary-actions">
            <Button asChild size="lg">
              <Link href={profileHref}>
                {user ? n('profile') : t('join')}
                <ArrowRight aria-hidden="true" />
              </Link>
            </Button>
            <Button asChild variant="outline" size="lg">
              <Link href="/connect">{t('connect')}</Link>
            </Button>
          </div>
        </div>
        <nav className="home-capabilities" aria-label={t('features')}>
          {capabilities.map((capability) => (
            <Link key={capability.title} href={capability.href}>
              <span>
                <strong>{t(capability.title)}</strong>
                <small>{t(capability.description)}</small>
              </span>
              <ArrowRight aria-hidden="true" />
            </Link>
          ))}
        </nav>
      </section>

      <section className="home-server-status" aria-label={t('status')} aria-busy={status.isLoading}>
        <div className="home-status-label">
          <span
            className={`home-status-dot${status.data && !status.error ? ' home-status-available' : ''}`}
            aria-hidden="true"
          />
          <span>
            {status.isLoading ? c('loading') : t(status.data && !status.error ? 'live' : 'offline')}
          </span>
          {status.error && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => status.mutate()}
              aria-label={t('retryStatus')}
            >
              <RefreshCw aria-hidden="true" />
              {c('retry')}
            </Button>
          )}
        </div>
        {status.data && !status.error && (
          <dl>
            {(
              [
                ['players', status.data.players_online],
                ['members', status.data.registered_players],
                ['scores', status.data.ranked_scores],
                ['beatmaps', status.data.beatmap_count],
              ] as const
            ).map(([key, value]) => (
              <div key={key}>
                <dt>{t(key)}</dt>
                <dd>{f.number(value)}</dd>
              </div>
            ))}
          </dl>
        )}
      </section>

      <div className="home-content-columns">
        <div className="home-main-content">
          <section aria-labelledby="home-maps-title" className="home-catalogue">
            <header className="home-section-heading">
              <div>
                <h2 id="home-maps-title">{t('explore')}</h2>
                <p>{t('exploreDescription')}</p>
              </div>
              <Link href="/beatmaps" className="text-link">
                {c('viewAll')}
                <ArrowRight aria-hidden="true" />
              </Link>
            </header>
            <div className="home-catalogue-panel glass">
              {maps.isLoading ? (
                <Loading />
              ) : maps.error ? (
                <LoadError retry={() => maps.mutate()} />
              ) : maps.data?.items.length ? (
                <MapRows items={maps.data.items} compact />
              ) : (
                <div className="home-empty">
                  <h3>{t('new')}</h3>
                  <p>{t('emptyMapsDescription')}</p>
                  <Link href="/beatmaps" className="text-link">
                    {t('maps')}
                    <ArrowRight aria-hidden="true" />
                  </Link>
                </div>
              )}
            </div>
          </section>

          <section aria-labelledby="home-scores-title" className="home-recent-scores">
            <header className="home-section-heading">
              <div>
                <h2 id="home-scores-title">{t('recentScores')}</h2>
                <p>{t('recentScoresDescription')}</p>
              </div>
            </header>
            <div className="home-score-panel glass">
              {scores.isLoading ? (
                <Loading />
              ) : scores.error ? (
                <LoadError retry={() => scores.mutate()} />
              ) : publicScores.length ? (
                <ol className="home-score-list">
                  {publicScores.map((score) => {
                    const mods = scoreMods(score);
                    return (
                      <li key={score.id}>
                        <div className="home-score-details">
                          <Link href={`/scores/${score.id}`} className="home-score-title">
                            {score.title}
                            <span>{score.version}</span>
                          </Link>
                          <div className="home-score-meta">
                            <Link
                              href={`/players/${score.user_id}?mode=${score.mode}&variant=${score.score_variant ?? 'vanilla'}`}
                            >
                              {score.username}
                            </Link>
                            <span>
                              {modes[score.mode]} ·{' '}
                              {c(score.score_variant === 'relax' ? 'relax' : 'vanilla')}
                            </span>
                            <span>
                              {mods.labels.join(' ') || 'NM'}
                              {mods.unknownBits ? ` + ${mods.unknownBits}` : ''}
                            </span>
                            {!score.passed && <span>{t('failedScore')}</span>}
                          </div>
                          <div className="home-score-secondary">
                            <Link href={`/beatmaps/${score.beatmap_id}`}>{t('viewBeatmap')}</Link>
                            <time dateTime={score.created_at}>
                              {f.dateTime(new Date(score.created_at), { dateStyle: 'medium' })}
                            </time>
                          </div>
                        </div>
                        <div className="home-score-values">
                          <strong>{f.number(score.pp, { maximumFractionDigits: 2 })} pp</strong>
                          <span>
                            {f.number(score.accuracy / 100, {
                              style: 'percent',
                              minimumFractionDigits: 2,
                              maximumFractionDigits: 2,
                            })}
                          </span>
                          {score.has_replay && (
                            <a
                              className="home-replay-link"
                              href={`/api/v1/scores/${score.id}/replay`}
                              aria-label={t('downloadReplay', {
                                title: score.title,
                                username: score.username,
                              })}
                            >
                              <Download aria-hidden="true" />
                              {c('replay')}
                            </a>
                          )}
                        </div>
                      </li>
                    );
                  })}
                </ol>
              ) : (
                <div className="home-empty">
                  <h3>{t('noRecentScores')}</h3>
                  <p>{t('noRecentScoresDescription')}</p>
                  <Link href="/connect" className="text-link">
                    {t('connect')}
                    <ArrowRight aria-hidden="true" />
                  </Link>
                </div>
              )}
            </div>
          </section>
        </div>

        <aside className="home-side-content">
          <AnnouncementsPreview />
          <section className="home-onboarding glass" aria-labelledby="home-start-title">
            <h2 id="home-start-title">{t('gettingStarted')}</h2>
            <ol>
              <li>
                <div>
                  <h3>{t('downloadClient')}</h3>
                  <p>{t('downloadClientDescription')}</p>
                  <a
                    href="https://osu.ppy.sh/home/download"
                    target="_blank"
                    rel="noreferrer"
                    className="text-link"
                  >
                    {t('officialDownload')}
                    <ArrowRight aria-hidden="true" />
                  </a>
                </div>
              </li>
              <li>
                <div>
                  <h3>{t('createAccountStep')}</h3>
                  <p>{t('createAccountDescription')}</p>
                  <Link href={profileHref} className="text-link">
                    {user ? n('profile') : t('join')}
                    <ArrowRight aria-hidden="true" />
                  </Link>
                </div>
              </li>
              <li>
                <div>
                  <h3>{t('connectStep')}</h3>
                  <p>{t('connectStepDescription')}</p>
                  <Link href="/connect" className="text-link">
                    {t('connect')}
                    <ArrowRight aria-hidden="true" />
                  </Link>
                </div>
              </li>
            </ol>
            <p className="home-connection-note">{t('notice')}</p>
          </section>

          <section className="home-ranking-preview" aria-labelledby="home-ranking-title">
            <header className="home-section-heading">
              <div>
                <h2 id="home-ranking-title">{t('leadingPlayers')}</h2>
                <p>osu! · {c('vanilla')}</p>
              </div>
              <Link href="/rankings?mode=0&variant=vanilla" className="text-link">
                {c('viewAll')}
                <ArrowRight aria-hidden="true" />
              </Link>
            </header>
            <div className="home-ranking-panel glass">
              {leaders.isLoading ? (
                <Loading />
              ) : leaders.error ? (
                <LoadError retry={() => leaders.mutate()} />
              ) : leaders.data?.items.length ? (
                <ol className="home-leader-list">
                  {leaders.data.items.map((player) => (
                    <li key={player.id}>
                      <span className="home-leader-rank">#{f.number(player.rank)}</span>
                      <Link
                        className="home-leader-player"
                        href={`/players/${player.id}?mode=0&variant=vanilla`}
                      >
                        <Avatar name={player.username} url={player.avatar_url} />
                        <span>
                          <strong>{player.username}</strong>
                          <small>
                            {player.country} · {f.number(player.pp, { maximumFractionDigits: 2 })}{' '}
                            pp
                          </small>
                        </span>
                      </Link>
                    </li>
                  ))}
                </ol>
              ) : (
                <div className="home-empty">
                  <h3>{t('noRankedPlayers')}</h3>
                  <p>{t('noRankedPlayersDescription')}</p>
                </div>
              )}
            </div>
          </section>
        </aside>
      </div>

      <section className="home-tools-directory" aria-labelledby="home-tools-title">
        <h2 id="home-tools-title">{t('tools')}</h2>
        <nav aria-label={t('tools')}>
          <Link href="/beatmaps">
            {n('beatmaps')}
            <ArrowRight aria-hidden="true" />
          </Link>
          <Link href="/rankings">
            {n('rankings')}
            <ArrowRight aria-hidden="true" />
          </Link>
          <Link href="/chat">
            {n('community')}
            <ArrowRight aria-hidden="true" />
          </Link>
          <Link href="/settings?tab=appearance">
            {n('appearance')}
            <ArrowRight aria-hidden="true" />
          </Link>
          {user && hasPermission(user.permissions, 'beatmaps.upload') && (
            <Link href="/beatmaps/upload">
              {m('upload')}
              <ArrowRight aria-hidden="true" />
            </Link>
          )}
          {user && hasPermission(user.permissions, 'beatmaps.import') && (
            <Link href="/beatmaps/import">
              {m('import')}
              <ArrowRight aria-hidden="true" />
            </Link>
          )}
          {user && (
            <>
              <Link href="/settings?tab=profile">
                {s('profile')}
                <ArrowRight aria-hidden="true" />
              </Link>
              <Link href="/settings?tab=account">
                {s('account')}
                <ArrowRight aria-hidden="true" />
              </Link>
              <Link href="/settings?tab=social">
                {social('title')}
                <ArrowRight aria-hidden="true" />
              </Link>
            </>
          )}
          {user && hasPermission(user.permissions, 'admin.view') && (
            <Link href="/admin">
              {n('admin')}
              <ArrowRight aria-hidden="true" />
            </Link>
          )}
        </nav>
      </section>
    </div>
  );
}
