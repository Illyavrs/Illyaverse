'use client';
import { useEffect, useState, type CSSProperties } from 'react';
import Link from 'next/link';
import { usePathname, useSearchParams } from 'next/navigation';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR, { unstable_serialize, useSWRConfig } from 'swr';
import { ArrowUpRight, Pencil } from 'lucide-react';
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
import type { Beatmap, BestScorePage, Leader, Page, User, Score, ScoreVariant } from '@/lib/types';
import { ProfileScoreList } from '@/components/profile-score-list';
import { MapRows } from './beatmaps';
import { RelationshipActions } from './social';
import { ScoreVariantControl, useScoreSelection } from '@/components/score-selection';
import { RankHistoryPanel } from './rank-history';

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
  const rankHistory = useTranslations('rankHistory');
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
  const uploadsPath = `/beatmaps?uploaded_by=${id}&source=local&page_size=5&page=1`;
  const uploadsKey =
    player?.id === id && !error && !sessionLoading
      ? ([uploadsPath, user?.id ?? 'guest'] as const)
      : null;
  const serializedUploadsKey = uploadsKey ? unstable_serialize(uploadsKey) : '';
  const {
    data: uploadedMaps,
    error: uploadsRequestError,
    mutate: reloadUploads,
  } = useSWR<Page<Beatmap>>(
    uploadsKey,
    (key: readonly [string, number | string]) => fetcher<Page<Beatmap>>(key[0]),
    {
      keepPreviousData: false,
      dedupingInterval: 0,
      shouldRetryOnError: (requestError) => !isProfileAccessError(requestError),
      onError: handleAccessError,
      onSuccess: (_data, requestedKey) => clearAccessFailure(requestedKey),
    },
  );
  const uploadsError = uploadsRequestError ?? accessFailures[serializedUploadsKey];
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
            <section
              className={`profile-masthead glass${player.banner_url ? ' has-banner' : ' no-banner'}`}
              id="profile-overview"
              aria-label={t('stats')}
            >
              <div className="profile-mode-rail">
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
              <div className="profile-identity">
                <Avatar name={player.username} url={player.avatar_url} size="large" />
                <div className="profile-player-name">
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
                <div className="profile-owner-actions">
                  {user?.id === id && (
                    <Button variant="outline" asChild>
                      <Link href="/settings?tab=profile">
                        <Pencil />
                        {t('edit')}
                      </Link>
                    </Button>
                  )}
                  <RelationshipActions target={id} targetUsername={player.username} />
                </div>
              </div>
              <div className="profile-statistics">
                <dl className="profile-stats">
                  <div>
                    <dt>pp</dt>
                    <dd>
                      {f.number(player.pp, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                    </dd>
                  </div>
                  <div>
                    <dt>{t('globalRank')}</dt>
                    <dd>
                      {player.statistics?.rank ? `#${f.number(player.statistics.rank)}` : '—'}
                    </dd>
                  </div>
                  <div>
                    <dt>{t('countryRank', { country: player.country })}</dt>
                    <dd>
                      {player.statistics?.country_rank
                        ? `#${f.number(player.statistics.country_rank)}`
                        : '—'}
                    </dd>
                  </div>
                </dl>
                <dl className="profile-detail-stats">
                  <div>
                    <dt>{c('accuracy')}</dt>
                    <dd>
                      {typeof player.statistics?.accuracy === 'number'
                        ? `${f.number(player.statistics.accuracy, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}%`
                        : '—'}
                    </dd>
                  </div>
                  <div>
                    <dt>{c('plays')}</dt>
                    <dd>{f.number(player.play_count)}</dd>
                  </div>
                  <div>
                    <dt>{t('maximumCombo')}</dt>
                    <dd>
                      {typeof player.statistics?.maximum_combo === 'number'
                        ? `${f.number(player.statistics.maximum_combo)}x`
                        : '—'}
                    </dd>
                  </div>
                  <div>
                    <dt>{t('totalHits')}</dt>
                    <dd>
                      {typeof player.statistics?.total_hits === 'number'
                        ? f.number(player.statistics.total_hits)
                        : '—'}
                    </dd>
                  </div>
                  <div>
                    <dt>{t('rankedScore')}</dt>
                    <dd>
                      {typeof player.statistics?.ranked_score === 'number'
                        ? f.number(player.statistics.ranked_score)
                        : '—'}
                    </dd>
                  </div>
                  <div>
                    <dt>{t('totalScore')}</dt>
                    <dd>
                      {typeof player.statistics?.total_score === 'number'
                        ? f.number(player.statistics.total_score)
                        : '—'}
                    </dd>
                  </div>
                </dl>
                {player.statistics?.score_scale === 'unavailable_for_relax' ? (
                  <p className="hint profile-totals-note">{t('relaxTotalsUnavailable')}</p>
                ) : (
                  player.statistics?.score_scale === 'classic_display' && (
                    <p className="hint profile-totals-note">
                      {t(
                        player.statistics.score_totals_complete
                          ? 'scoreTotalsClassic'
                          : 'scoreTotalsPartial',
                      )}
                    </p>
                  )
                )}
                {player.statistics?.grade_counts && (
                  <div className="profile-grade-strip" aria-describedby="profile-grades-hint">
                    <h2>{t('gradeCounts')}</h2>
                    <dl>
                      {(['ssh', 'ss', 'sh', 's', 'a', 'b', 'c', 'd', 'f'] as const).map((grade) => (
                        <div key={grade}>
                          <dt>
                            {grade === 'ssh' ? 'SS+' : grade === 'sh' ? 'S+' : grade.toUpperCase()}
                          </dt>
                          <dd>
                            {typeof player.statistics?.grade_counts?.[grade] === 'number'
                              ? f.number(player.statistics.grade_counts[grade]!)
                              : '—'}
                          </dd>
                        </div>
                      ))}
                    </dl>
                    <p className="hint" id="profile-grades-hint">
                      {t('gradeCountsHint')}
                    </p>
                  </div>
                )}
              </div>
            </section>
            <nav className="profile-section-nav" aria-label={t('sections')}>
              <a href="#profile-overview">{t('stats')}</a>
              <a href="#profile-rank-history">{rankHistory('title')}</a>
              <a href="#profile-about">{t('about')}</a>
              <a href="#profile-scores">{t('scoreView')}</a>
              <a href="#profile-uploads">{t('uploadedMaps')}</a>
            </nav>
            <div className="profile-layout">
              <RankHistoryPanel id={id} mode={mode} variant={variant} />
              <section
                className="profile-about glass"
                id="profile-about"
                aria-labelledby="profile-about-heading"
              >
                <h2 id="profile-about-heading">{t('about')}</h2>
                <p className="profile-bio">{player.bio || t('noBio')}</p>
              </section>
              <section
                className="content-panel glass profile-scores"
                id="profile-scores"
                aria-labelledby="profile-scores-title"
              >
                <header className="section-heading profile-scores-header">
                  <div className="profile-scores-heading">
                    <h2 id="profile-scores-title">{t(view === 'best' ? 'best' : 'scores')}</h2>
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
                    <ProfileScoreList scores={scores.items} best={view === 'best'} />
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
              <section
                className="content-panel glass profile-uploads"
                id="profile-uploads"
                aria-labelledby="profile-uploads-title"
              >
                <header className="section-heading">
                  <div className="profile-scores-heading">
                    <h2 id="profile-uploads-title">{t('uploadedMaps')}</h2>
                    {uploadedMaps && !uploadsError && (
                      <Badge variant="secondary">{f.number(uploadedMaps.total)}</Badge>
                    )}
                  </div>
                  <Button asChild variant="ghost">
                    <Link href={`/beatmaps?uploaded_by=${id}&source=local`}>
                      {t('viewUploadedMaps')}
                      <ArrowUpRight />
                    </Link>
                  </Button>
                </header>
                <p className="hint">{t('uploadedMapsHint')}</p>
                {uploadsError ? (
                  <LoadError retry={() => reloadUploads()} />
                ) : !uploadedMaps ? (
                  <Loading />
                ) : uploadedMaps.items.length ? (
                  <MapRows items={uploadedMaps.items} compact />
                ) : (
                  <Empty title={t('emptyUploadedMaps')} />
                )}
              </section>
            </div>
          </>
        )
      )}
    </div>
  );
}
