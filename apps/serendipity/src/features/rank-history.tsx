'use client';

import { useState, type KeyboardEvent, type PointerEvent } from 'react';
import { useSearchParams } from 'next/navigation';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR, { useSWRConfig } from 'swr';
import { RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Loading } from '@/components/common';
import { useSession } from '@/components/providers';
import { ApiError, fetcher } from '@/lib/api';

export type RankHistoryPoint = {
  date: string;
  captured_at: string;
  pp: number;
  rank: number | null;
  country_rank: number | null;
  country: string;
};
export type RankHistoryResponse = {
  mode: number;
  variant: 'vanilla' | 'relax';
  days: number;
  start_date: string;
  end_date: string;
  time_zone: 'UTC';
  collection_interval_seconds: number;
  last_captured_at: string | null;
  points: RankHistoryPoint[];
};
type Metric = 'global' | 'country';
const dayMilliseconds = 86_400_000;
const plotTop = 14;
const plotBottom = 190;

function utcDate(value: string) {
  return new Date(`${value}T00:00:00Z`);
}
function validDate(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    /^\d{4}-\d{2}-\d{2}$/.test(value) &&
    Number.isFinite(utcDate(value).getTime()) &&
    utcDate(value).toISOString().slice(0, 10) === value
  );
}
function validRank(value: unknown) {
  return value === null || (typeof value === 'number' && Number.isSafeInteger(value) && value > 0);
}
function validHistory(data: RankHistoryResponse, mode: string, variant: string, days: number) {
  if (
    !data ||
    data.mode !== Number(mode) ||
    data.variant !== variant ||
    data.days !== days ||
    data.time_zone !== 'UTC' ||
    !validDate(data.start_date) ||
    !validDate(data.end_date) ||
    utcDate(data.end_date).getTime() - utcDate(data.start_date).getTime() !==
      (days - 1) * dayMilliseconds ||
    !Number.isSafeInteger(data.collection_interval_seconds) ||
    data.collection_interval_seconds <= 0 ||
    (data.last_captured_at !== null && !Number.isFinite(Date.parse(data.last_captured_at))) ||
    !Array.isArray(data.points)
  )
    return false;
  return data.points.every(
    (point, index) =>
      point &&
      validDate(point.date) &&
      point.date >= data.start_date &&
      point.date <= data.end_date &&
      (!index || data.points[index - 1].date < point.date) &&
      typeof point.captured_at === 'string' &&
      Number.isFinite(Date.parse(point.captured_at)) &&
      typeof point.pp === 'number' &&
      Number.isFinite(point.pp) &&
      point.pp >= 0 &&
      validRank(point.rank) &&
      validRank(point.country_rank) &&
      typeof point.country === 'string',
  );
}
function placement(point: RankHistoryPoint, metric: Metric) {
  return point.pp > 0 ? (metric === 'country' ? point.country_rank : point.rank) : null;
}
function selectControl(name: string, value: string) {
  const query = new URLSearchParams(window.location.search);
  query.set(name, value);
  window.history.pushState(null, '', `${window.location.pathname}?${query}${window.location.hash}`);
}

export function RankHistoryPanel({
  id,
  mode,
  variant,
}: {
  id: number;
  mode: string;
  variant: string;
}) {
  const t = useTranslations('rankHistory');
  const search = useSearchParams();
  const { user, loading } = useSession();
  const rawDays = Number(search.get('rank_days') ?? 90);
  const days = [30, 90, 180].includes(rawDays) ? rawDays : 90;
  const metric: Metric = search.get('rank_metric') === 'country' ? 'country' : 'global';
  const path = `/users/${id}/rank-history?${new URLSearchParams({ mode, variant, days: String(days) })}`;
  const viewer = user?.id ?? 'guest';
  return (
    <section
      className="rank-history-panel glass"
      id="profile-rank-history"
      aria-labelledby="rank-history-title"
    >
      <header className="rank-history-header">
        <h2 id="rank-history-title">{t('title')}</h2>
        <div className="rank-history-controls">
          <label>
            <span>{t('placement')}</span>
            <select
              className="native-select"
              value={metric}
              onChange={(event) => selectControl('rank_metric', event.target.value)}
            >
              <option value="global">{t('global')}</option>
              <option value="country">{t('country')}</option>
            </select>
          </label>
          <label>
            <span>{t('period')}</span>
            <select
              className="native-select"
              value={days}
              onChange={(event) => selectControl('rank_days', event.target.value)}
            >
              {[30, 90, 180].map((value) => (
                <option key={value} value={value}>
                  {t('days', { days: value })}
                </option>
              ))}
            </select>
          </label>
        </div>
      </header>
      {loading ? (
        <Loading />
      ) : (
        <RankHistoryContent
          key={`${path}:${viewer}`}
          path={path}
          viewer={viewer}
          mode={mode}
          variant={variant}
          days={days}
          metric={metric}
        />
      )}
    </section>
  );
}

function RankHistoryContent({
  path,
  viewer,
  mode,
  variant,
  days,
  metric,
}: {
  path: string;
  viewer: number | string;
  mode: string;
  variant: string;
  days: number;
  metric: Metric;
}) {
  const t = useTranslations('rankHistory');
  const c = useTranslations('common');
  const f = useFormatter();
  const { mutate: mutateCache } = useSWRConfig();
  const [verified, setVerified] = useState(false);
  const [accessFailure, setAccessFailure] = useState<ApiError | null>(null);
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const { data, error, isLoading, isValidating, mutate } = useSWR<RankHistoryResponse>(
    [path, viewer],
    async (key: readonly [string, number | string]) => {
      const response = await fetcher<RankHistoryResponse>(key[0]);
      if (!validHistory(response, mode, variant, days))
        throw new Error('Invalid rank history response');
      return response;
    },
    {
      keepPreviousData: false,
      revalidateOnMount: true,
      dedupingInterval: 0,
      shouldRetryOnError: false,
      onSuccess: () => {
        setVerified(true);
        setAccessFailure(null);
      },
      onError: (requestError, requestedKey) => {
        if (requestError instanceof ApiError && [401, 403, 404].includes(requestError.status)) {
          setVerified(false);
          setAccessFailure(requestError);
          void mutateCache(requestedKey, undefined, { revalidate: false });
        }
      },
    },
  );
  const failure = error ?? accessFailure;
  if (failure) {
    const message =
      failure instanceof ApiError
        ? failure.status === 401
          ? 'signIn'
          : failure.status === 403
            ? 'forbidden'
            : failure.status === 404
              ? 'notFound'
              : 'error'
        : 'error';
    return (
      <div className="rank-history-state" role="alert">
        <p>{t(message)}</p>
        <Button variant="outline" disabled={isValidating} onClick={() => void mutate()}>
          <RefreshCw />
          {c('retry')}
        </Button>
      </div>
    );
  }
  if (isLoading || !verified || !data) return <Loading />;
  const formatDate = (date: string) =>
    f.dateTime(utcDate(date), { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' });
  const rankText = (rank: number | null) => (rank === null ? t('unranked') : `#${f.number(rank)}`);
  const selectedIndex = data.points.findIndex((point) => point.date === selectedDate);
  const index = selectedIndex >= 0 ? selectedIndex : data.points.length - 1;
  const selected = data.points[index];
  const captureHint = data.last_captured_at
    ? t('lastCaptured', {
        date: f.dateTime(new Date(data.last_captured_at), {
          dateStyle: 'medium',
          timeStyle: 'short',
          timeZone: 'UTC',
        }),
      })
    : t('notCaptured');
  if (!selected)
    return (
      <div className="rank-history-state">
        <p className="rank-history-empty-title">{t('emptyTitle')}</p>
        <p>{t('emptyDescription')}</p>
        <p className="hint">
          {t('cadence', { minutes: data.collection_interval_seconds / 60 })} {captureHint}
        </p>
      </div>
    );

  const ranked = data.points
    .map((point) => placement(point, metric))
    .filter((rank): rank is number => rank !== null);
  const best = ranked.length ? Math.min(...ranked) : 1;
  const worst = ranked.length ? Math.max(...ranked) : 1;
  const padding = Math.max(1, Math.ceil((worst - best) * 0.12));
  const upper = Math.max(1, best - padding);
  const lower = worst + padding;
  const x = (point: RankHistoryPoint) =>
    4 +
    ((utcDate(point.date).getTime() - utcDate(data.start_date).getTime()) /
      ((days - 1) * dayMilliseconds)) *
      992;
  const y = (rank: number) => plotTop + ((rank - upper) / (lower - upper)) * (plotBottom - plotTop);
  const selectionText = (point: RankHistoryPoint) =>
    t('pointDescription', {
      date: formatDate(point.date),
      rank: rankText(placement(point, metric)),
      country: metric === 'country' ? point.country : t('global'),
      pp: f.number(point.pp, { maximumFractionDigits: 2 }),
    });
  function handleKeys(event: KeyboardEvent<HTMLDivElement>) {
    let next = index;
    if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') next = Math.max(0, index - 1);
    else if (event.key === 'ArrowRight' || event.key === 'ArrowUp')
      next = Math.min(data!.points.length - 1, index + 1);
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = data!.points.length - 1;
    else return;
    event.preventDefault();
    setSelectedDate(data!.points[next].date);
  }
  function handlePointer(event: PointerEvent<SVGSVGElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    const coordinate = ((event.clientX - bounds.left) / bounds.width) * 1000;
    const nearest = data!.points.reduce((closest, point) =>
      Math.abs(x(point) - coordinate) < Math.abs(x(closest) - coordinate) ? point : closest,
    );
    setSelectedDate(nearest.date);
    event.currentTarget.parentElement?.focus({ preventScroll: true });
  }
  return (
    <>
      <dl className="rank-history-summary" aria-live="polite" aria-atomic="true">
        <div>
          <dt>{t(selectedIndex >= 0 ? 'selectedDay' : 'latestDay')}</dt>
          <dd>
            <time dateTime={selected.date}>{formatDate(selected.date)}</time>
          </dd>
        </div>
        <div>
          <dt>
            {metric === 'country'
              ? t('sampledCountryRank', { country: selected.country })
              : t('global')}
          </dt>
          <dd>{rankText(placement(selected, metric))}</dd>
        </div>
        <div>
          <dt>{t('performance')}</dt>
          <dd>
            {f.number(selected.pp, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} pp
          </dd>
        </div>
      </dl>
      <figure className="rank-history-figure">
        <div
          className="rank-history-chart"
          role="slider"
          tabIndex={0}
          aria-label={t('chartLabel', { metric: t(metric) })}
          aria-valuemin={1}
          aria-valuemax={data.points.length}
          aria-valuenow={index + 1}
          aria-valuetext={selectionText(selected)}
          aria-describedby="rank-history-caption rank-history-keyboard"
          onKeyDown={handleKeys}
        >
          <div className="rank-history-axis" aria-hidden="true">
            {ranked.length ? (
              [upper, Math.round((upper + lower) / 2), lower].map((rank, tick) => (
                <span key={tick} style={{ top: `${(y(rank) / 220) * 100}%` }}>
                  {rankText(rank)}
                </span>
              ))
            ) : (
              <span className="rank-history-unranked-axis">{t('unranked')}</span>
            )}
          </div>
          <svg
            viewBox="0 0 1000 220"
            preserveAspectRatio="none"
            aria-hidden="true"
            onPointerDown={handlePointer}
          >
            {[plotTop, (plotTop + plotBottom) / 2, plotBottom].map((value) => (
              <line
                key={value}
                className="rank-history-grid"
                x1="0"
                x2="1000"
                y1={value}
                y2={value}
                vectorEffect="non-scaling-stroke"
              />
            ))}
            {data.points.map((point, pointIndex) => {
              const previous = data.points[pointIndex - 1];
              const rank = placement(point, metric);
              const previousRank = previous && placement(previous, metric);
              const adjacent =
                previous &&
                utcDate(point.date).getTime() - utcDate(previous.date).getTime() ===
                  dayMilliseconds;
              return rank !== null &&
                previousRank !== null &&
                previousRank !== undefined &&
                adjacent &&
                (metric !== 'country' || previous.country === point.country) ? (
                <line
                  key={point.date}
                  className="rank-history-segment"
                  data-start-date={previous.date}
                  data-end-date={point.date}
                  x1={x(previous)}
                  x2={x(point)}
                  y1={y(previousRank)}
                  y2={y(rank)}
                  vectorEffect="non-scaling-stroke"
                />
              ) : null;
            })}
            <line
              className="rank-history-selection"
              x1={x(selected)}
              x2={x(selected)}
              y1="0"
              y2="220"
              vectorEffect="non-scaling-stroke"
            />
            {data.points.map((point) => {
              const rank = placement(point, metric);
              return rank === null ? (
                <line
                  key={point.date}
                  className={`rank-history-null${point.date === selected.date ? ' is-selected' : ''}`}
                  data-date={point.date}
                  x1={x(point)}
                  x2={x(point)}
                  y1="209"
                  y2="217"
                  vectorEffect="non-scaling-stroke"
                />
              ) : (
                <circle
                  key={point.date}
                  className={`rank-history-point${point.date === selected.date ? ' is-selected' : ''}`}
                  data-date={point.date}
                  cx={x(point)}
                  cy={y(rank)}
                  r={point.date === selected.date ? 4 : 2}
                  vectorEffect="non-scaling-stroke"
                />
              );
            })}
          </svg>
        </div>
        <div className="rank-history-dates" aria-hidden="true">
          <time dateTime={data.start_date}>{formatDate(data.start_date)}</time>
          <time dateTime={data.end_date}>{formatDate(data.end_date)}</time>
        </div>
        <figcaption id="rank-history-caption">
          {data.points.length === 1 ? `${t('singleDay')} ` : ''}
          {t(metric === 'country' ? 'countryGaps' : 'gaps')}
        </figcaption>
      </figure>
      <p className="hint rank-history-keyboard" id="rank-history-keyboard">
        {t('keyboard')}
      </p>
      <p className="hint rank-history-cadence">
        {t('cadence', { minutes: data.collection_interval_seconds / 60 })} {captureHint}
      </p>
      <details className="rank-history-details">
        <summary>{t('dailyTable', { count: data.points.length })}</summary>
        <div
          className="rank-history-table-scroll"
          tabIndex={0}
          role="region"
          aria-label={t('tableLabel')}
        >
          <table className="data-table rank-history-table">
            <caption>{t('tableCaption')}</caption>
            <thead>
              <tr>
                <th scope="col">{t('date')}</th>
                <th scope="col">{t('performance')}</th>
                <th scope="col">{t('global')}</th>
                <th scope="col">{t('country')}</th>
                <th scope="col">{t('sampledCountry')}</th>
                <th scope="col">{t('captured')}</th>
              </tr>
            </thead>
            <tbody>
              {data.points.map((point) => (
                <tr
                  key={point.date}
                  className={point.date === selected.date ? 'is-selected' : undefined}
                >
                  <th scope="row">
                    <button
                      type="button"
                      aria-pressed={point.date === selected.date}
                      onClick={() => setSelectedDate(point.date)}
                    >
                      <time dateTime={point.date}>{formatDate(point.date)}</time>
                    </button>
                  </th>
                  <td>
                    {f.number(point.pp, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} pp
                  </td>
                  <td>{rankText(placement(point, 'global'))}</td>
                  <td>{rankText(placement(point, 'country'))}</td>
                  <td>{point.country}</td>
                  <td>
                    <time dateTime={point.captured_at}>
                      {f.dateTime(new Date(point.captured_at), {
                        hour: '2-digit',
                        minute: '2-digit',
                        timeZone: 'UTC',
                      })}{' '}
                      UTC
                    </time>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>
    </>
  );
}
