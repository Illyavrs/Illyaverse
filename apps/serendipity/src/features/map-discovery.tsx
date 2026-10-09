'use client';

import { useEffect, useId, useMemo, useState, type CSSProperties, type FormEvent } from 'react';
import { NavigationLink as Link, useNavigationProgress } from '@/components/navigation-motion';
import Image from 'next/image';
import { MapCardPreview } from './map-card-preview';
import { OfficialMapDiscoveryPage } from './public-official-maps';
import { useSearchParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import {
  ArrowLeft,
  ArrowRight,
  Clock,
  Grid2X2,
  Library,
  List,
  Plus,
  RefreshCw,
  Search,
  Star,
  X,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldSelect } from '@/components/ui/field-select';
import { PageHeading } from '@/components/common';
import { useSession } from '@/components/providers';
import { api, ApiError, hasPermission } from '@/lib/api';
import type { Beatmap, BeatmapDifficulty, Page, ScoreVariant } from '@/lib/types';

// This mirrors the public set response; server filters select matching difficulties first.
export type DiscoverySet = {
  native_set_id: number;
  representative: Beatmap;
  matching_count: number;
  stored_count: number;
  matching_modes: number[];
  matching_statuses: string[];
  stars_min: number;
  stars_max: number;
  length_min: number;
  length_max: number;
  added_at: string;
  preview: BeatmapDifficulty[];
};

const modes = ['osu!', 'taiko', 'catch', 'mania'] as const;
const statuses = ['ranked', 'loved', 'pending', 'unranked'] as const;
const sources = ['official', 'local'] as const;
const sorts = ['recent', 'title', 'stars', 'length'] as const;
const filterKeys = ['q', 'mode', 'status', 'source', 'sort', 'uploaded_by', 'page'] as const;
const returnLimit = 2048;

export function safeCatalogueReturn(value: string | null) {
  if (
    !value ||
    !value.startsWith('/') ||
    value.startsWith('//') ||
    new TextEncoder().encode(value).length > returnLimit
  )
    return null;
  try {
    const url = new URL(value, 'https://illya.invalid');
    const result = `${url.pathname}${url.search}`;
    return url.origin === 'https://illya.invalid' &&
      url.pathname === '/beatmaps' &&
      new TextEncoder().encode(result).length <= returnLimit
      ? result
      : null;
  } catch {
    return null;
  }
}

type DifficultyNavigation = {
  returnTo?: string | null;
  mode?: string;
  variant?: ScoreVariant;
  management?: boolean;
};
export function beatmapDifficultyHref(
  map: BeatmapDifficulty,
  navigation: DifficultyNavigation = {},
) {
  const query = new URLSearchParams();
  if (navigation.mode !== undefined || navigation.variant !== undefined) {
    const mode =
      map.mode === 0 && /^[0-3]$/.test(navigation.mode ?? '') ? navigation.mode! : String(map.mode);
    query.set('mode', mode);
    query.set('variant', mode === '3' ? 'vanilla' : (navigation.variant ?? 'vanilla'));
  }
  if (navigation.returnTo) query.set('returnTo', navigation.returnTo);
  if (navigation.management) query.set('management', '1');
  return `/beatmaps/${map.id}${query.size ? `?${query}` : ''}`;
}

export function boundMapSearch(value: string) {
  let result = '';
  let bytes = 0;
  const encoder = new TextEncoder();
  for (const character of value) {
    const length = encoder.encode(character).length;
    if (bytes + length > 100) break;
    result += character;
    bytes += length;
  }
  return result;
}

function positiveInteger(value: string | null) {
  if (!value || !/^\d+$/.test(value)) return null;
  const number = Number(value);
  return Number.isSafeInteger(number) && number > 0 ? number : null;
}

export function mapDuration(seconds: number) {
  const safe = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  return `${Math.floor(safe / 60)}:${String(safe % 60).padStart(2, '0')}`;
}

export function mapStarRange(min: number, max: number) {
  return min === max ? min.toFixed(2) : `${min.toFixed(2)}–${max.toFixed(2)}`;
}

export function mapCoverUrl(source: string, nativeSetId?: number) {
  return source === 'official' &&
    Number.isSafeInteger(nativeSetId) &&
    nativeSetId! > 0 &&
    nativeSetId! < 2_000_000_000
    ? `https://assets.ppy.sh/beatmaps/${nativeSetId}/covers/cover.jpg`
    : null;
}

export function MapArtwork({
  source,
  nativeSetId,
  className = '',
  priority = false,
}: {
  source: string;
  nativeSetId?: number;
  className?: string;
  priority?: boolean;
}) {
  const t = useTranslations('mapDiscovery');
  const src = mapCoverUrl(source, nativeSetId);
  const [failed, setFailed] = useState<string | null>(null);
  return (
    <span className={`map-artwork ${className}`}>
      {src && failed !== src ? (
        <Image
          src={src}
          alt=""
          width={600}
          height={160}
          unoptimized
          loading={priority ? 'eager' : 'lazy'}
          onError={() => setFailed(src)}
        />
      ) : (
        <span className="map-artwork-fallback">
          <Library aria-hidden="true" />
          <span>{t(source === 'local' ? 'localArtwork' : 'coverUnavailable')}</span>
        </span>
      )}
    </span>
  );
}

export function MapMetadata({ map }: { map: Beatmap }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  return (
    <dl className="discovery-detail-metadata">
      {[
        [t('stars'), map.stars.toFixed(2)],
        [t('bpm'), map.bpm.toFixed(0)],
        [t('length'), mapDuration(map.length_seconds)],
        [c('combo'), `${map.max_combo}x`],
      ].map(([label, value], index) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>
            {index === 0 && <Star aria-hidden="true" />}
            {value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function difficultyLevel(stars: number) {
  return stars < 2
    ? 'easy'
    : stars < 2.7
      ? 'normal'
      : stars < 4
        ? 'hard'
        : stars < 5.3
          ? 'insane'
          : stars < 6.5
            ? 'expert'
            : 'extreme';
}

export function DifficultyChip({
  map,
  current = false,
  compact = false,
  navigation,
}: {
  map: BeatmapDifficulty;
  current?: boolean;
  compact?: boolean;
  navigation?: DifficultyNavigation;
}) {
  const t = useTranslations('mapDiscovery');
  const mode = modes[map.mode] ?? t('unknownMode');
  const style = {
    '--difficulty-color': `var(--difficulty-${difficultyLevel(map.stars)})`,
  } as CSSProperties;
  return (
    <Link
      href={beatmapDifficultyHref(map, navigation)}
      className={`difficulty-chip${compact ? ' compact' : ''}`}
      style={style}
      aria-current={current ? 'page' : undefined}
      aria-label={t('openDifficulty', { version: map.version, mode, stars: map.stars.toFixed(2) })}
      title={`${map.version} · ${mode} · ${map.stars.toFixed(2)} ★`}
    >
      <span className="difficulty-marker" aria-hidden="true" />
      <span className="difficulty-chip-copy">
        <span className="difficulty-mode">{mode}</span>
        <span className="difficulty-version">{map.version}</span>
      </span>
      <span className="difficulty-rating">
        <Star aria-hidden="true" />
        {map.stars.toFixed(2)}
      </span>
    </Link>
  );
}

// Fresh requests make the public access boundary explicit. Responses from old keys never render.
export function usePublicData<T>(path: string) {
  const { beginRequest } = useNavigationProgress();
  const [attempt, setAttempt] = useState(0);
  const request = useMemo(() => ({ path, attempt }), [path, attempt]);
  const [response, setResponse] = useState<{
    request: typeof request;
    data?: T;
    error?: unknown;
  } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    const finish = beginRequest();
    api<T>(request.path, { signal: controller.signal }).then(
      (data) => {
        if (!controller.signal.aborted) setResponse({ request, data });
        finish();
      },
      (error: unknown) => {
        if (!controller.signal.aborted) setResponse({ request, error });
        finish();
      },
    );
    return () => {
      controller.abort();
      finish();
    };
  }, [request, beginRequest]);
  const current = response?.request === request ? response : null;
  return {
    data: current?.data,
    error: current?.error,
    loading: !current,
    retry: () => setAttempt((value) => value + 1),
  };
}

function StatusLabel({ value }: { value: string }) {
  const t = useTranslations('mapDiscovery');
  const known = statuses.includes(value as (typeof statuses)[number]);
  return (
    <span className={`discovery-status${known ? ` status-${value}` : ''}`}>
      {known ? t(`status.${value}`) : value}
    </span>
  );
}

function DiscoveryPagination({
  page,
  total,
  pageSize,
  onChange,
}: {
  page: number;
  total: number;
  pageSize: number;
  onChange: (page: number) => void;
}) {
  const t = useTranslations('mapDiscovery');
  const pages = Math.max(1, Math.ceil(total / pageSize));
  return pages > 1 ? (
    <nav className="discovery-pagination" aria-label={t('pagination')}>
      <Button
        type="button"
        variant="outline"
        disabled={page <= 1}
        onClick={() => onChange(page - 1)}
      >
        <ArrowLeft aria-hidden="true" />
        {t('previous')}
      </Button>
      <span>{t('pageNumber', { page, pages })}</span>
      <Button
        type="button"
        variant="outline"
        disabled={page >= pages}
        onClick={() => onChange(page + 1)}
      >
        {t('next')}
        <ArrowRight aria-hidden="true" />
      </Button>
    </nav>
  ) : null;
}

function DiscoveryError({
  retry,
  withdrawn = false,
  contained = false,
}: {
  retry: () => void;
  withdrawn?: boolean;
  contained?: boolean;
}) {
  const t = useTranslations('mapDiscovery');
  return (
    <div className={`discovery-feedback${contained ? '' : ' glass'}`} role="alert">
      <h2>{t(withdrawn ? 'withdrawnTitle' : 'loadErrorTitle')}</h2>
      <p>{t(withdrawn ? 'withdrawnHint' : 'loadErrorHint')}</p>
      {!withdrawn && (
        <Button type="button" variant="outline" onClick={retry}>
          <RefreshCw aria-hidden="true" />
          {t('retry')}
        </Button>
      )}
    </div>
  );
}

function DiscoverySkeleton({ compact = false }: { compact?: boolean }) {
  const t = useTranslations('mapDiscovery');
  return (
    <div
      className={`discovery-skeleton${compact ? ' difficulty-loading' : ''}`}
      role="status"
      aria-label={t('loading')}
    >
      <span />
      <span />
      {!compact && (
        <>
          <span />
          <span />
        </>
      )}
    </div>
  );
}

function MapSetCard({ set, returnTo }: { set: DiscoverySet; returnTo: string }) {
  const t = useTranslations('mapDiscovery');
  const map = set.representative;
  const cover = mapCoverUrl(map.source, set.native_set_id);
  const mappedBy = t('mappedBy', { creator: map.creator });
  const matchingModes = set.matching_modes
    .map((mode) => modes[mode] ?? t('unknownMode'))
    .join(' / ');
  const length =
    set.length_min === set.length_max
      ? mapDuration(set.length_min)
      : `${mapDuration(set.length_min)}–${mapDuration(set.length_max)}`;
  return (
    <article
      className="discovery-set glass"
      style={cover ? ({ '--catalogue-cover': `url("${cover}")` } as CSSProperties) : undefined}
    >
      <Link href={beatmapDifficultyHref(map, { returnTo })} className="discovery-set-main">
        <MapArtwork source={map.source} nativeSetId={set.native_set_id} />
        <div className="discovery-set-copy">
          <h2 title={map.title}>{map.title}</h2>
          <span className="discovery-artist" title={map.artist}>
            {map.artist}
          </span>
          <p title={mappedBy}>{mappedBy}</p>
          <div className="discovery-set-meta">
            <span className="discovery-stars">
              <Star aria-hidden="true" />
              {mapStarRange(set.stars_min, set.stars_max)}
            </span>
            <span>
              <Clock aria-hidden="true" />
              {length}
            </span>
            <span className="discovery-modes" title={matchingModes}>
              {matchingModes}
            </span>
          </div>
        </div>
        <ArrowRight className="discovery-open" aria-hidden="true" />
      </Link>
      <div className="discovery-difficulties">
        <div className="discovery-set-statuses">
          {set.matching_statuses.map((status) => (
            <StatusLabel value={status} key={status} />
          ))}
          {map.source === 'local' && <span className="discovery-source">{t('source.local')}</span>}
        </div>
        <MapCardPreview set={set} />
      </div>
    </article>
  );
}

function SearchField({ value, onSearch }: { value: string; onSearch: (value: string) => void }) {
  const t = useTranslations('mapDiscovery');
  const [draft, setDraft] = useState(value);
  const id = useId();
  function submit(event: FormEvent) {
    event.preventDefault();
    onSearch(boundMapSearch(draft).trim());
  }
  return (
    <form className="discovery-search" onSubmit={submit}>
      <label htmlFor={id}>{t('searchLabel')}</label>
      <div>
        <div className="search-input">
          <Search aria-hidden="true" />
          <Input
            id={id}
            value={draft}
            onChange={(event) => setDraft(boundMapSearch(event.target.value))}
            placeholder={t('searchPlaceholder')}
          />
        </div>
        <Button type="submit">{t('searchAction')}</Button>
      </div>
    </form>
  );
}

export function CatalogueSource({ value }: { value: 'official' | 'server' }) {
  const t = useTranslations('publicOfficialMaps');
  return (
    <nav className="catalogue-source" aria-label={t('sourceLabel')}>
      <Link href="/beatmaps" aria-current={value === 'official' ? 'page' : undefined}>
        {t('official')}
      </Link>
      <Link
        href="/beatmaps?catalogue=server"
        aria-current={value === 'server' ? 'page' : undefined}
      >
        {t('server')}
      </Link>
    </nav>
  );
}

export function MapDiscoveryPage() {
  const params = useSearchParams();
  // Existing profile/uploader links continue to open the server catalogue.
  const server =
    params.get('catalogue') === 'server' || params.has('uploaded_by') || params.has('source');
  return server ? <ServerMapDiscoveryPage /> : <OfficialMapDiscoveryPage />;
}

function ServerMapDiscoveryPage() {
  const t = useTranslations('mapDiscovery');
  const maps = useTranslations('maps');
  const { user } = useSession();
  const params = useSearchParams();
  const search = boundMapSearch(params.get('q') ?? '');
  const mode = ['0', '1', '2', '3'].includes(params.get('mode') ?? '')
    ? params.get('mode')!
    : 'all';
  const status = statuses.includes(params.get('status') as (typeof statuses)[number])
    ? params.get('status')!
    : 'all';
  const source = sources.includes(params.get('source') as (typeof sources)[number])
    ? params.get('source')!
    : 'all';
  const sort = sorts.includes(params.get('sort') as (typeof sorts)[number])
    ? params.get('sort')!
    : 'recent';
  const page = positiveInteger(params.get('page')) ?? 1;
  const uploader = positiveInteger(params.get('uploaded_by'));
  const view = params.get('view') === 'list' ? 'list' : 'grid';
  const returnParams = new URLSearchParams(params);
  returnParams.set('catalogue', 'server');
  const returnTo = safeCatalogueReturn(`/beatmaps?${returnParams}`) ?? '/beatmaps?catalogue=server';
  const query = new URLSearchParams({ page: String(page), page_size: '24', sort });
  if (search) query.set('q', search);
  if (mode !== 'all') query.set('mode', mode);
  if (status !== 'all') query.set('status', status);
  if (source !== 'all') query.set('source', source);
  if (uploader) query.set('uploaded_by', String(uploader));
  const { data, error, loading, retry } = usePublicData<Page<DiscoverySet>>(
    `/beatmapsets?${query}`,
  );
  function update(key: string, value: string | null, resetPage = true) {
    const next = new URLSearchParams(window.location.search);
    next.set('catalogue', 'server');
    if (resetPage) next.delete('page');
    if (value === null || value === '' || value === 'all') next.delete(key);
    else next.set(key, value);
    window.history.pushState(null, '', `${window.location.pathname}${next.size ? `?${next}` : ''}`);
  }
  function reset() {
    const next = new URLSearchParams(window.location.search);
    next.set('catalogue', 'server');
    filterKeys.forEach((key) => next.delete(key));
    window.history.pushState(null, '', `${window.location.pathname}${next.size ? `?${next}` : ''}`);
  }
  const filtered =
    !!search ||
    mode !== 'all' ||
    status !== 'all' ||
    source !== 'all' ||
    !!uploader ||
    sort !== 'recent' ||
    page !== 1;
  const outOfRange = data && page > Math.max(1, Math.ceil(data.total / data.page_size));
  return (
    <div className="page-container map-discovery">
      <div className="discovery-reading-ground" data-material="section">
        <CatalogueSource value="server" />
        <PageHeading title={maps('title')} description={t('description')}>
          {user && hasPermission(user.permissions, 'beatmaps.upload') && (
            <Button asChild>
              <Link href="/beatmaps/upload">
                <Plus aria-hidden="true" />
                {maps('upload')}
              </Link>
            </Button>
          )}
          {user && hasPermission(user.permissions, 'beatmaps.import') && (
            <Button asChild variant="outline">
              <Link href="/beatmaps/import">{maps('import')}</Link>
            </Button>
          )}
        </PageHeading>
        <section className="discovery-controls" aria-label={t('filters')}>
          <SearchField key={search} value={search} onSearch={(value) => update('q', value)} />
          <div className="discovery-filter-row">
            <div className="field">
              <label htmlFor="discovery-mode">{t('modeLabel')}</label>
              <FieldSelect
                id="discovery-mode"
                className="native-select"
                value={mode}
                onValueChange={(value) => update('mode', value)}
                options={[
                  { value: 'all', label: t('allModes') },
                  ...modes.map((label, index) => ({ value: String(index), label })),
                ]}
              />
            </div>
            <div className="field">
              <label htmlFor="discovery-status">{t('statusLabel')}</label>
              <FieldSelect
                id="discovery-status"
                className="native-select"
                value={status}
                onValueChange={(value) => update('status', value)}
                options={[
                  { value: 'all', label: t('allStatuses') },
                  ...statuses.map((value) => ({ value, label: t(`status.${value}`) })),
                ]}
              />
            </div>
            <div className="field">
              <label htmlFor="discovery-source">{t('sourceLabel')}</label>
              <FieldSelect
                id="discovery-source"
                className="native-select"
                value={source}
                onValueChange={(value) => update('source', value)}
                options={[
                  { value: 'all', label: t('allSources') },
                  ...sources.map((value) => ({ value, label: t(`source.${value}`) })),
                ]}
              />
            </div>
            <div className="field">
              <label htmlFor="discovery-sort">{t('sortLabel')}</label>
              <FieldSelect
                id="discovery-sort"
                className="native-select"
                value={sort}
                onValueChange={(value) => update('sort', value)}
                options={sorts.map((value) => ({ value, label: t(`sort.${value}`) }))}
              />
            </div>
          </div>
          {uploader && (
            <div className="discovery-uploader">
              <span>{t('uploaderFilter', { id: uploader })}</span>
              <Button variant="ghost" type="button" onClick={() => update('uploaded_by', null)}>
                <X aria-hidden="true" />
                {t('clearUploader')}
              </Button>
            </div>
          )}
        </section>
      </div>
      <div className="discovery-result-bar">
        <div className="discovery-result-count" role="status" aria-live="polite">
          {loading ? t('loading') : !error && data ? t('setCount', { count: data.total }) : ''}
          {!loading && !error && data && data.total > 0 && !outOfRange && (
            <span>
              {t('resultRange', {
                first: (data.page - 1) * data.page_size + 1,
                last: Math.min(data.page * data.page_size, data.total),
              })}
            </span>
          )}
        </div>
        <div className="discovery-result-actions">
          {filtered && (
            <Button variant="ghost" type="button" onClick={reset}>
              {t('resetFilters')}
            </Button>
          )}
          <div className="discovery-view-control" role="group" aria-label={t('viewLabel')}>
            <button
              type="button"
              aria-pressed={view === 'grid'}
              onClick={() => update('view', 'grid', false)}
            >
              <Grid2X2 aria-hidden="true" />
              {t('grid')}
            </button>
            <button
              type="button"
              aria-pressed={view === 'list'}
              onClick={() => update('view', 'list', false)}
            >
              <List aria-hidden="true" />
              {t('list')}
            </button>
          </div>
        </div>
      </div>
      <section
        className={`discovery-results view-${view}`}
        aria-label={t('resultsLabel')}
        aria-busy={loading}
      >
        {loading ? (
          <DiscoverySkeleton />
        ) : error ? (
          <DiscoveryError retry={retry} />
        ) : outOfRange ? (
          <div className="discovery-feedback glass">
            <h2>{t('pageMissingTitle')}</h2>
            <p>{t('pageMissingHint')}</p>
            <Button type="button" variant="outline" onClick={() => update('page', null, false)}>
              {t('firstPage')}
            </Button>
          </div>
        ) : !data?.items.length ? (
          <div className="discovery-feedback glass">
            <h2>{t(filtered ? 'emptyFilteredTitle' : 'emptyLibraryTitle')}</h2>
            <p>{t(filtered ? 'emptyFilteredHint' : 'emptyLibraryHint')}</p>
            {filtered && (
              <Button type="button" variant="outline" onClick={reset}>
                {t('resetFilters')}
              </Button>
            )}
          </div>
        ) : (
          data.items.map((set) => (
            <MapSetCard set={set} key={set.native_set_id} returnTo={returnTo} />
          ))
        )}
      </section>
      {data && !error && !outOfRange && (
        <DiscoveryPagination
          page={data.page}
          total={data.total}
          pageSize={data.page_size}
          onChange={(value) => update('page', String(value), false)}
        />
      )}
    </div>
  );
}

type SetDifficultySelectorProps = {
  mapId: number;
  currentVersion?: string;
  navigation?: DifficultyNavigation;
};
export function SetDifficultySelector(props: SetDifficultySelectorProps) {
  return <SetDifficultySelectorContent key={props.mapId} {...props} />;
}

function SetDifficultySelectorContent({
  mapId,
  currentVersion,
  navigation,
}: SetDifficultySelectorProps) {
  const t = useTranslations('mapDiscovery');
  const [page, setPage] = useState(1);
  const titleId = useId();
  const { data, error, loading, retry } = usePublicData<Page<BeatmapDifficulty>>(
    `/beatmaps/${mapId}/set/difficulties?page=${page}&page_size=24`,
  );
  const withdrawn = error instanceof ApiError && error.status === 410;
  const outOfRange = data && page > Math.max(1, Math.ceil(data.total / data.page_size));
  return (
    <section
      id="set-difficulties"
      className="set-difficulty-selector map-package glass"
      aria-labelledby={titleId}
      aria-busy={loading}
    >
      <header className="set-difficulty-heading">
        <div>
          <h2 id={titleId}>{t('setTitle')}</h2>
          <p>{data ? t('knownCount', { count: data.total }) : t('loading')}</p>
        </div>
        {currentVersion && (
          <p className="set-current">
            {t('currentDifficulty')}
            <strong>{currentVersion}</strong>
          </p>
        )}
      </header>
      <p className="set-difficulty-hint">{t('knownHint')}</p>
      {loading ? (
        <DiscoverySkeleton compact />
      ) : error ? (
        <DiscoveryError withdrawn={withdrawn} retry={retry} contained />
      ) : outOfRange ? (
        <div className="discovery-feedback">
          <p>{t('pageMissingHint')}</p>
          <Button type="button" variant="outline" onClick={() => setPage(1)}>
            {t('firstPage')}
          </Button>
        </div>
      ) : !data?.items.length ? (
        <p>{t('noKnownDifficulties')}</p>
      ) : (
        <>
          <div
            className="difficulty-strip set-difficulty-strip"
            role="region"
            tabIndex={0}
            aria-label={t('difficultyQuickLinks')}
          >
            {data.items.map((map) => (
              <DifficultyChip
                key={map.id}
                map={map}
                current={map.id === mapId}
                navigation={navigation}
              />
            ))}
          </div>
          <details className="set-difficulty-disclosure" key={page}>
            <summary className="set-difficulty-summary">{t('difficultyDetails')}</summary>
            <div className="set-difficulty-rows package-difficulty-list">
              {data.items.map((map) => (
                <Link
                  href={beatmapDifficultyHref(map, navigation)}
                  key={map.id}
                  className="set-difficulty-row package-difficulty-row"
                  aria-current={map.id === mapId ? 'page' : undefined}
                  style={
                    {
                      '--difficulty-color': `var(--difficulty-${difficultyLevel(map.stars)})`,
                    } as CSSProperties
                  }
                >
                  <span className="set-difficulty-name map-main">
                    <strong>{map.version}</strong>
                    <span>{t('mappedBy', { creator: map.creator })}</span>
                  </span>
                  <span className="set-difficulty-details">
                    <span>{modes[map.mode] ?? t('unknownMode')}</span>
                    <StatusLabel value={map.status} />
                    <span>
                      {map.source === 'local'
                        ? t('source.local')
                        : map.source === 'official'
                          ? t('source.official')
                          : map.source}
                    </span>
                  </span>
                  <span className="set-difficulty-numbers">
                    <span className="difficulty-rating">
                      <Star aria-hidden="true" />
                      {map.stars.toFixed(2)}
                    </span>
                    <span>
                      <Clock aria-hidden="true" />
                      {mapDuration(map.length_seconds)}
                    </span>
                  </span>
                  {map.id === mapId && <span className="set-current-label">{t('current')}</span>}
                  <ArrowRight className="set-difficulty-arrow" aria-hidden="true" />
                </Link>
              ))}
            </div>
          </details>
        </>
      )}
      {data && !error && !outOfRange && (
        <DiscoveryPagination
          page={data.page}
          total={data.total}
          pageSize={data.page_size}
          onChange={setPage}
        />
      )}
    </section>
  );
}
