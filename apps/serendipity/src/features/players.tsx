'use client';
import { useState } from 'react';
import Link from 'next/link';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR from 'swr';
import { Pencil } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  PageHeading,
  GameMode,
  Loading,
  LoadError,
  Empty,
  Pagination,
  Avatar,
} from '@/components/common';
import { useSession } from '@/components/providers';
import { fetcher } from '@/lib/api';
import type { Leader, Page, User, Score } from '@/lib/types';
import { ScoreTable } from './scores';
import { RelationshipActions } from './social';
import { ScoreVariantControl, useScoreSelection } from '@/components/score-selection';
export function RankingsPage() {
  const t = useTranslations('rankings');
  const c = useTranslations('common');
  const f = useFormatter();
  const { mode, variant, select, href } = useScoreSelection();
  const [country, setCountry] = useState('');
  const [page, setPage] = useState(1);
  const query = new URLSearchParams({ mode, variant, page: String(page), page_size: '24' });
  if (country.length === 2) query.set('country', country);
  const { data, error, isLoading, mutate } = useSWR<Page<Leader>>(`/leaderboard?${query}`, fetcher);
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')} />
      <div className="filter-bar glass">
        <GameMode
          value={mode}
          onChange={(v) => {
            select(v, variant);
            setPage(1);
          }}
        />
        <ScoreVariantControl mode={mode} value={variant} onChange={(v) => { select(mode, v); setPage(1); }} />
        <input
          className="native-select country-input"
          placeholder="TW"
          maxLength={2}
          aria-label={t('country')}
          value={country}
          onChange={(e) => {
            setCountry(e.target.value.toUpperCase().replace(/[^A-Z]/g, ''));
            setPage(1);
          }}
        />
        <p className="hint">{t('modeNotice')}</p>
      </div>
      {mode === '3' && <p className="hint">{c('relaxUnsupported')}</p>}
      <section className="glass content-panel">
        {isLoading ? (
          <Loading />
        ) : error ? (
          <LoadError retry={() => mutate()} />
        ) : !data?.items.length ? (
          <Empty title={t('emptyTitle')} description={t('emptyDescription')} />
        ) : (
          <div className="table-scroll">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{t('rank')}</th>
                  <th>{t('player')}</th>
                  <th>{c('country')}</th>
                  <th>{t('performance')}</th>
                  <th>{c('accuracy')}</th>
                  <th>{t('playCount')}</th>
                </tr>
              </thead>
              <tbody>
                {data.items.map((player) => (
                  <tr key={player.id}>
                    <td className="rank-cell">#{player.rank}</td>
                    <td>
                      <Link href={href(`/players/${player.id}`)} className="player-cell">
                        <Avatar name={player.username} url={player.avatar_url} />
                        {player.username}
                      </Link>
                    </td>
                    <td>{player.country}</td>
                    <td className="pp-value">
                      {f.number(player.pp, { maximumFractionDigits: 2 })} pp
                    </td>
                    <td>{player.accuracy.toFixed(2)}%</td>
                    <td>{f.number(player.play_count)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
      {data && <Pagination page={page} total={data.total} onChange={setPage} />}
    </div>
  );
}
export function PlayerPage({ id }: { id: number }) {
  const t = useTranslations('profile');
  const c = useTranslations('common');
  const f = useFormatter();
  const { user } = useSession();
  const { mode, variant, select, selection } = useScoreSelection();
  const [page, setPage] = useState(1);
  const { data: player, error, isLoading, mutate } = useSWR<User>(`/users/${id}?${selection}`, fetcher);
  const query = new URLSearchParams({ user_id: String(id), mode, variant, page: String(page), page_size: '24' });
  const {
    data: scores,
    error: scoreError,
    mutate: reloadScores,
  } = useSWR<Page<Score>>(player ? `/scores?${query}` : null, fetcher);
  return (
    <div className="page-container">
      {isLoading ? (
        <Loading />
      ) : error ? (
        <LoadError retry={() => mutate()} />
      ) : (
        player && (
          <>
            <div
              className="profile-cover"
              style={{
                backgroundImage: player.banner_url
                  ? `url(${JSON.stringify(player.banner_url)})`
                  : undefined,
              }}
              role={player.banner_url ? 'img' : undefined}
              aria-label={player.banner_url ? t('banner') : undefined}
            />
            <section className="profile-identity">
              <Avatar name={player.username} url={player.avatar_url} size="large" />
              <div>
                <h1>{player.username}</h1>
                <p>
                  {player.country} <span className="dot">·</span> {t('joined')}{' '}
                  {f.dateTime(new Date(player.joined_at), { dateStyle: 'medium' })}
                </p>
                <div className="map-tags">
                  {player.roles.map((role) => (
                    <Badge variant="secondary" key={role}>
                      {role}
                    </Badge>
                  ))}
                </div>
              </div>
              {user?.id === id && (
                <Button variant="outline" asChild>
                  <Link href="/settings">
                    <Pencil />
                    {t('edit')}
                  </Link>
                </Button>
              )}
            </section>
            <RelationshipActions target={id} />
            <div className="filter-bar glass">
              <GameMode value={mode} onChange={(v) => { select(v, variant); setPage(1); }} />
              <ScoreVariantControl mode={mode} value={variant} onChange={(v) => { select(mode, v); setPage(1); }} />
              {mode === '3' && <p className="hint">{c('relaxUnsupported')}</p>}
            </div>
            <div className="profile-layout">
              <aside className="profile-about glass">
                <h2>{t('about')}</h2>
                <p className="profile-bio">{player.bio || t('noBio')}</p>
                <dl className="profile-stats">
                  <div>
                    <dt>pp</dt>
                    <dd>{f.number(player.pp, { maximumFractionDigits: 2 })}</dd>
                  </div>
                  <div>
                    <dt>{c('plays')}</dt>
                    <dd>{f.number(player.play_count)}</dd>
                  </div>
                  {player.statistics && <>
                    <div><dt>{t('globalRank')}</dt><dd>{player.statistics.rank ? `#${f.number(player.statistics.rank)}` : '—'}</dd></div>
                    <div><dt>{c('accuracy')}</dt><dd>{f.number(player.statistics.accuracy, { maximumFractionDigits: 2 })}%</dd></div>
                  </>}
                </dl>
              </aside>
              <section className="content-panel glass">
                <header className="section-heading profile-scores-header">
                  <h2>{t('scores')}</h2>
                  <Badge variant="secondary">{c(variant)}</Badge>
                </header>
                {scoreError ? (
                  <LoadError retry={() => reloadScores()} />
                ) : !scores ? (
                  <Loading />
                ) : scores.items.length ? (
                  <>
                    <ScoreTable scores={scores.items} />
                    <Pagination page={page} total={scores.total} onChange={setPage} />
                  </>
                ) : (
                  <Empty title={t('emptyScores')} />
                )}
              </section>
            </div>
          </>
        )
      )}
    </div>
  );
}
