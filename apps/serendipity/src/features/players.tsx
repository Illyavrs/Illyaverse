'use client';
import { useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { usePathname, useSearchParams } from 'next/navigation';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR, { unstable_serialize, useSWRConfig } from 'swr';
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
import { ApiError, fetcher } from '@/lib/api';
import type { BestScorePage, Leader, Page, User, Score, ScoreVariant } from '@/lib/types';
import { ScoreTable } from './scores';
import { RelationshipActions } from './social';
import { ScoreVariantControl, useScoreSelection } from '@/components/score-selection';

function profilePresentation(player: User): CSSProperties {
  const accent =
    typeof player.accent_color === 'string' && /^#[0-9a-f]{6}$/i.test(player.accent_color)
      ? player.accent_color
      : '#f28eb6';
  const rgb = accent
    .slice(1)
    .match(/.{2}/g)!
    .map((value) => parseInt(value, 16) / 255)
    .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
  const luminance = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
  const darkLuminance = ((17 / 255 + 0.055) / 1.055) ** 2.4;
  const darkContrast = (luminance + 0.05) / (darkLuminance + 0.05);
  const lightContrast = 1.05 / (luminance + 0.05);
  return {
    '--profile-accent': accent,
    '--profile-accent-contrast':
      Math.max(darkContrast, lightContrast) < 4.5
        ? '#000000'
        : darkContrast >= lightContrast
          ? '#111111'
          : '#ffffff',
  } as CSSProperties;
}

function bannerPosition(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 100
    ? value
    : 50;
}

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
        <ScoreVariantControl
          mode={mode}
          value={variant}
          onChange={(v) => {
            select(mode, v);
            setPage(1);
          }}
        />
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
type ProfileScoreView = 'best' | 'recent';
type ProfileScoreSelection = {
  mode: string;
  variant: ScoreVariant;
  view: ProfileScoreView;
  page: number;
};

function readProfileScoreSelection(query: Pick<URLSearchParams, 'get'>): ProfileScoreSelection {
  const rawMode = query.get('mode') ?? '0';
  const mode = /^[0-3]$/.test(rawMode) ? rawMode : '0';
  const rawPage = query.get('page') ?? '1';
  const page = /^\d+$/.test(rawPage) ? Number(rawPage) : 1;
  return {
    mode,
    variant: mode !== '3' && query.get('variant') === 'relax' ? 'relax' : 'vanilla',
    view: query.get('score_view') === 'recent' ? 'recent' : 'best',
    page: Number.isSafeInteger(page) ? Math.max(1, Math.min(page, 1_000_000)) : 1,
  };
}

function updateProfileScoreSelection(change: Partial<ProfileScoreSelection>, replace = false) {
  // Read live URL state so sequential collection/mode/variant changes compose before React renders.
  const query = new URLSearchParams(window.location.search);
  const next = { ...readProfileScoreSelection(query), ...change };
  if (next.mode === '3') next.variant = 'vanilla';
  next.page = Number.isSafeInteger(next.page) ? Math.max(1, Math.min(next.page, 1_000_000)) : 1;
  if (change.mode !== undefined || change.variant !== undefined || change.view !== undefined)
    next.page = 1;
  query.set('mode', next.mode);
  query.set('variant', next.variant);
  if (next.view === 'recent') query.set('score_view', 'recent');
  else query.delete('score_view');
  if (next.page > 1) query.set('page', String(next.page));
  else query.delete('page');
  const url = `${window.location.pathname}?${query}${window.location.hash}`;
  if (replace) window.history.replaceState(null, '', url);
  else window.history.pushState(null, '', url);
}

export function ProfileScoreCollectionControl({
  value,
  onChange,
}: {
  value: ProfileScoreView;
  onChange: (value: ProfileScoreView) => void;
}) {
  const t = useTranslations('profile');
  return (
    <div className="mode-control profile-score-collection" role="group" aria-label={t('scoreView')}>
      {(['best', 'recent'] as const).map((view) => (
        <button
          key={view}
          type="button"
          aria-pressed={value === view}
          onClick={() => onChange(view)}
        >
          {t(view === 'best' ? 'best' : 'scores')}
        </button>
      ))}
    </div>
  );
}

function isProfileAccessError(error: unknown): error is ApiError {
  return error instanceof ApiError && [401, 403, 404].includes(error.status);
}

export function PlayerPage({ id }: { id: number }) {
  const t = useTranslations('profile');
  const c = useTranslations('common');
  const f = useFormatter();
  const { user, loading: sessionLoading } = useSession();
  const { mutate: mutateCache } = useSWRConfig();
  const search = useSearchParams();
  const pathname = usePathname();
  const { mode, variant, view, page } = readProfileScoreSelection(search);
  const selection = new URLSearchParams({ mode, variant });
  const profileKey = `/users/${id}?${selection}`;
  const [accessFailures, setAccessFailures] = useState<Record<string, ApiError>>({});
  function clearAccessFailure(key: string) {
    setAccessFailures((current) => {
      if (!current[key]) return current;
      const next = { ...current };
      delete next[key];
      return next;
    });
  }
  function handleAccessError(requestError: unknown, requestedKey: string) {
    if (!isProfileAccessError(requestError)) return;
    setAccessFailures((current) => ({ ...current, [requestedKey]: requestError }));
    // Purge the precise failed key, even if a newer profile selection is already active.
    void mutateCache(requestedKey, undefined, { revalidate: false });
  }
  const {
    data: player,
    error: playerError,
    isLoading,
    mutate,
  } = useSWR<User>(profileKey, fetcher, {
    keepPreviousData: false,
    shouldRetryOnError: (requestError) => !isProfileAccessError(requestError),
    onError: handleAccessError,
    onSuccess: (_data, requestedKey) => clearAccessFailure(requestedKey),
  });
  const error = playerError ?? accessFailures[profileKey];
  const query = new URLSearchParams({
    mode,
    variant,
    page: String(page),
    page_size: '24',
  });
  const bestPath = `/users/${id}/scores/best?${query}`;
  query.set('user_id', String(id));
  const recentPath = `/scores?${query}`;
  const scoreKey =
    player?.id === id && !error && !(view === 'recent' && sessionLoading)
      ? view === 'best'
        ? bestPath
        : ([recentPath, user?.id ?? 'guest'] as const)
      : null;
  const serializedScoreKey = scoreKey ? unstable_serialize(scoreKey) : '';
  const {
    data: scores,
    error: scoreRequestError,
    mutate: reloadScores,
  } = useSWR<BestScorePage | Page<Score>>(
    scoreKey,
    (key: string | readonly [string, number | string]) =>
      fetcher<BestScorePage | Page<Score>>(typeof key === 'string' ? key : key[0]),
    {
      keepPreviousData: false,
      // Returning to a private recent collection must recheck access immediately.
      dedupingInterval: 0,
      shouldRetryOnError: (requestError) => !isProfileAccessError(requestError),
      onError: handleAccessError,
      onSuccess: (_data, requestedKey) => clearAccessFailure(requestedKey),
    },
  );
  const scoreError = scoreRequestError ?? accessFailures[serializedScoreKey];
  useEffect(() => {
    const live = readProfileScoreSelection(new URLSearchParams(window.location.search));
    if (
      window.location.pathname !== pathname ||
      live.mode !== mode ||
      live.variant !== variant ||
      live.view !== view ||
      live.page !== page
    )
      return;
    const lastPage =
      scores && !scoreError ? Math.max(1, Math.ceil(scores.total / scores.page_size)) : page;
    const boundedPage = Math.min(page, lastPage);
    const rawPage = search.get('page');
    if (boundedPage !== page || (rawPage !== null && rawPage !== String(boundedPage)))
      updateProfileScoreSelection({ page: boundedPage }, true);
  }, [pathname, mode, variant, view, page, search, scores, scoreError]);
  return (
    <div
      className={`page-container profile-page${player ? ' profile-accent-scope' : ''}`}
      style={player ? profilePresentation(player) : undefined}
    >
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
                backgroundPosition: `${bannerPosition(player.banner_position_x)}% ${bannerPosition(player.banner_position_y)}%`,
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
                  <Link href="/settings?tab=profile">
                    <Pencil />
                    {t('edit')}
                  </Link>
                </Button>
              )}
            </section>
            <RelationshipActions target={id} />
            <div className="filter-bar glass">
              <GameMode
                value={mode}
                onChange={(nextMode) => updateProfileScoreSelection({ mode: nextMode })}
              />
              <ScoreVariantControl
                mode={mode}
                value={variant}
                onChange={(nextVariant) => updateProfileScoreSelection({ variant: nextVariant })}
              />
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
                  {player.statistics && (
                    <>
                      <div>
                        <dt>{t('globalRank')}</dt>
                        <dd>
                          {player.statistics.rank ? `#${f.number(player.statistics.rank)}` : '—'}
                        </dd>
                      </div>
                      <div>
                        <dt>{c('accuracy')}</dt>
                        <dd>
                          {f.number(player.statistics.accuracy, { maximumFractionDigits: 2 })}%
                        </dd>
                      </div>
                    </>
                  )}
                </dl>
              </aside>
              <section className="content-panel glass">
                <header className="section-heading profile-scores-header">
                  <div className="profile-scores-heading">
                    <h2>{t(view === 'best' ? 'best' : 'scores')}</h2>
                    <Badge variant="secondary">{c(variant)}</Badge>
                  </div>
                  <ProfileScoreCollectionControl
                    value={view}
                    onChange={(nextView) => updateProfileScoreSelection({ view: nextView })}
                  />
                </header>
                <p className="hint profile-scores-description">
                  {t(view === 'best' ? 'bestHint' : 'recentHint')}
                </p>
                {view === 'recent' && sessionLoading ? (
                  <Loading />
                ) : scoreError ? (
                  <LoadError retry={() => reloadScores()} />
                ) : !scores ? (
                  <Loading />
                ) : scores.items.length ? (
                  <>
                    <ScoreTable scores={scores.items} profileBest={view === 'best'} />
                    <Pagination
                      page={page}
                      total={scores.total}
                      pageSize={scores.page_size}
                      onChange={(nextPage) => updateProfileScoreSelection({ page: nextPage })}
                    />
                  </>
                ) : (
                  <Empty
                    title={t(view === 'best' ? 'emptyBestTitle' : 'emptyScores')}
                    description={view === 'best' ? t('emptyBestDescription') : undefined}
                  />
                )}
              </section>
            </div>
          </>
        )
      )}
    </div>
  );
}
