'use client';

import { useId, useState, type CSSProperties, type FormEvent } from 'react';
import { useSearchParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import {
  ArrowLeft,
  ArrowRight,
  Clock,
  Download,
  ExternalLink,
  Grid2X2,
  List,
  Plus,
  RefreshCw,
  Search,
  Star,
} from 'lucide-react';
import { NavigationLink as Link } from '@/components/navigation-motion';
import { Button } from '@/components/ui/button';
import { FieldSelect } from '@/components/ui/field-select';
import { Input } from '@/components/ui/input';
import { PageHeading } from '@/components/common';
import { useSession } from '@/components/providers';
import { ApiError, hasPermission } from '@/lib/api';
import type { OfficialBeatmapSearch, OfficialBeatmapset } from '@/lib/types';
import { MapCardPreview } from './map-card-preview';
import {
  CatalogueSource,
  MapArtwork,
  difficultyLevel,
  mapDuration,
  mapStarRange,
  safeCatalogueReturn,
  usePublicData,
} from './map-discovery';

const modes = ['osu!', 'taiko', 'catch', 'mania'];
const statuses = ['ranked', 'qualified', 'loved', 'pending', 'graveyard', 'any'];
const sorts = ['ranked_desc', 'date_desc', 'title_asc', 'difficulty_desc'];

export function boundedOfficialQuery(value: string) {
  // The Rust API applies the same 200-character/800-byte bound independently.
  return Array.from(value.replace(/[\u0000-\u001f\u007f-\u009f]/g, ' '))
    .slice(0, 200)
    .join('');
}

function updateCatalogue(key: string, value: string, resetCursor = true) {
  const next = new URLSearchParams(window.location.search);
  if (resetCursor) next.delete('cursor');
  if (!value || (key === 'mode' && value === 'all')) next.delete(key);
  else next.set(key, value);
  window.history.pushState(null, '', `/beatmaps${next.size ? `?${next}` : ''}`);
}

function OfficialSearchField({ value }: { value: string }) {
  const t = useTranslations('mapDiscovery');
  const id = useId();
  const [draft, setDraft] = useState(value);
  function submit(event: FormEvent) {
    event.preventDefault();
    updateCatalogue('q', boundedOfficialQuery(draft).trim());
  }
  return (
    <form className="discovery-search" onSubmit={submit}>
      <label htmlFor={id}>{t('searchLabel')}</label>
      <div>
        <div className="search-input">
          <Search aria-hidden="true" />
          <Input
            id={id}
            type="search"
            value={draft}
            onChange={(event) => setDraft(boundedOfficialQuery(event.target.value))}
            placeholder={t('searchPlaceholder')}
          />
        </div>
        <Button type="submit">{t('searchAction')}</Button>
      </div>
    </form>
  );
}

function OfficialStatus({ value }: { value: OfficialBeatmapset['status'] }) {
  const t = useTranslations('maps.officialSearch');
  return <span className={`discovery-status status-${value}`}>{t(`statuses.${value}`)}</span>;
}

function OfficialSetCard({ set, returnTo }: { set: OfficialBeatmapset; returnTo: string }) {
  const t = useTranslations('mapDiscovery');
  const stars = set.difficulties.map((map) => map.stars);
  const lengths = set.difficulties.map((map) => map.length_seconds);
  const modeNames = [...new Set(set.difficulties.map((map) => modes[map.mode]))].join(' / ');
  const lengthMin = Math.min(...lengths);
  const lengthMax = Math.max(...lengths);
  const mappedBy = t('mappedBy', { creator: set.creator });
  return (
    <article
      className="discovery-set glass"
      style={{ '--catalogue-cover': `url("${set.cover_url}")` } as CSSProperties}
    >
      <Link
        href={`/beatmaps/official/${set.official_set_id}?returnTo=${encodeURIComponent(returnTo)}`}
        className="discovery-set-main"
      >
        <MapArtwork source="official" nativeSetId={set.official_set_id} />
        <div className="discovery-set-copy">
          <h2 title={set.title}>{set.title}</h2>
          <span className="discovery-artist" title={set.artist}>
            {set.artist}
          </span>
          <p title={mappedBy}>{mappedBy}</p>
          <div className="discovery-set-meta">
            <span className="discovery-stars">
              <Star aria-hidden="true" />
              {mapStarRange(Math.min(...stars), Math.max(...stars))}
            </span>
            <span>
              <Clock aria-hidden="true" />
              {lengthMin === lengthMax
                ? mapDuration(lengthMin)
                : `${mapDuration(lengthMin)}–${mapDuration(lengthMax)}`}
            </span>
            <span className="discovery-modes" title={modeNames}>
              {modeNames}
            </span>
          </div>
        </div>
        <ArrowRight className="discovery-open" aria-hidden="true" />
      </Link>
      <div className="discovery-difficulties">
        <div className="discovery-set-statuses">
          <OfficialStatus value={set.status} />
        </div>
        <MapCardPreview official={set} />
      </div>
    </article>
  );
}

function OfficialFeedback({ error, retry }: { error: unknown; retry: () => void }) {
  const t = useTranslations('publicOfficialMaps');
  const c = useTranslations('mapDiscovery');
  const upstream = useTranslations('maps.officialSearch');
  const withdrawn = error instanceof ApiError && [404, 410].includes(error.status);
  const limited = error instanceof ApiError && error.status === 429;
  const hint =
    error instanceof ApiError && error.code === 'official_api_not_configured'
      ? upstream('errorConfig')
      : error instanceof ApiError && error.code === 'official_auth_failed'
        ? upstream('errorAuth')
        : error instanceof ApiError && error.code === 'official_search_timeout'
          ? upstream('errorTimeout')
          : t(limited ? 'busyHint' : 'errorHint');
  return (
    <div className="discovery-feedback glass" role="alert">
      <h2>{withdrawn ? c('withdrawnTitle') : t('errorTitle')}</h2>
      <p>{withdrawn ? c('withdrawnHint') : hint}</p>
      {!withdrawn && (
        <Button type="button" variant="outline" onClick={retry}>
          <RefreshCw aria-hidden="true" />
          {c('retry')}
        </Button>
      )}
    </div>
  );
}

export function OfficialMapDiscoveryPage() {
  const t = useTranslations('publicOfficialMaps');
  const c = useTranslations('mapDiscovery');
  const maps = useTranslations('maps');
  const upstream = useTranslations('maps.officialSearch');
  const { user } = useSession();
  const params = useSearchParams();
  const q = boundedOfficialQuery(params.get('q') ?? '');
  const mode = /^[0-3]$/.test(params.get('mode') ?? '') ? params.get('mode')! : 'all';
  const status = statuses.includes(params.get('status') ?? '') ? params.get('status')! : 'ranked';
  const sort = sorts.includes(params.get('sort') ?? '') ? params.get('sort')! : 'ranked_desc';
  const cursor = params.get('cursor');
  const safeCursor =
    cursor && cursor.length <= 2048 && /^[\x20-\x7e]+$/.test(cursor) ? cursor : null;
  const view = params.get('view') === 'list' ? 'list' : 'grid';
  const query = new URLSearchParams({ q, mode, status, sort });
  if (safeCursor) query.set('cursor_string', safeCursor);
  const { data, error, loading, retry } = usePublicData<OfficialBeatmapSearch>(
    `/official/beatmapsets/search?${query}`,
  );
  const returnTo =
    safeCatalogueReturn(`/beatmaps${params.size ? `?${params}` : ''}`) ?? '/beatmaps';
  const filtered =
    !!q || mode !== 'all' || status !== 'ranked' || sort !== 'ranked_desc' || !!safeCursor;
  function reset() {
    const next = new URLSearchParams();
    if (view === 'list') next.set('view', view);
    window.history.pushState(null, '', `/beatmaps${next.size ? `?${next}` : ''}`);
  }
  function changePage(value: string | null) {
    const next = new URLSearchParams(window.location.search);
    if (value) next.set('cursor', value);
    else next.delete('cursor');
    window.history.pushState(null, '', `/beatmaps${next.size ? `?${next}` : ''}`);
  }
  return (
    <div className="page-container map-discovery public-official-maps">
      <div className="discovery-reading-ground" data-material="section">
        <CatalogueSource value="official" />
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
        <section className="discovery-controls" aria-label={c('filters')}>
          <OfficialSearchField key={q} value={q} />
          <div className="discovery-filter-row">
            <div className="field">
              <label htmlFor="official-mode">{c('modeLabel')}</label>
              <FieldSelect
                id="official-mode"
                className="native-select"
                value={mode}
                onValueChange={(value) => updateCatalogue('mode', value)}
                options={[
                  { value: 'all', label: c('allModes') },
                  ...modes.map((label, index) => ({ value: String(index), label })),
                ]}
              />
            </div>
            <div className="field">
              <label htmlFor="official-status">{c('statusLabel')}</label>
              <FieldSelect
                id="official-status"
                className="native-select"
                value={status}
                onValueChange={(value) => updateCatalogue('status', value)}
                options={statuses.map((value) => ({
                  value,
                  label: value === 'any' ? upstream('allStatuses') : upstream(`statuses.${value}`),
                }))}
              />
            </div>
            <div className="field">
              <label htmlFor="official-sort">{c('sortLabel')}</label>
              <FieldSelect
                id="official-sort"
                className="native-select"
                value={sort}
                onValueChange={(value) => updateCatalogue('sort', value)}
                options={sorts.map((value) => ({ value, label: upstream(`sorts.${value}`) }))}
              />
            </div>
          </div>
        </section>
      </div>
      <div className="discovery-result-bar">
        <div className="discovery-result-count" role="status" aria-live="polite">
          {loading ? c('loading') : !error && data ? c('setCount', { count: data.total }) : ''}
          {!loading && !error && data && (
            <span>{t('showingCount', { count: data.items.length })}</span>
          )}
        </div>
        <div className="discovery-result-actions">
          {filtered && (
            <Button type="button" variant="ghost" onClick={reset}>
              {c('resetFilters')}
            </Button>
          )}
          <div className="discovery-view-control" role="group" aria-label={c('viewLabel')}>
            <button
              type="button"
              aria-pressed={view === 'grid'}
              onClick={() => updateCatalogue('view', 'grid', false)}
            >
              <Grid2X2 aria-hidden="true" />
              {c('grid')}
            </button>
            <button
              type="button"
              aria-pressed={view === 'list'}
              onClick={() => updateCatalogue('view', 'list', false)}
            >
              <List aria-hidden="true" />
              {c('list')}
            </button>
          </div>
        </div>
      </div>
      <section
        className={`discovery-results view-${view}`}
        aria-label={c('resultsLabel')}
        aria-busy={loading}
      >
        {loading ? (
          <div className="discovery-skeleton" role="status" aria-label={c('loading')}>
            <span />
            <span />
            <span />
            <span />
          </div>
        ) : error ? (
          <OfficialFeedback error={error} retry={retry} />
        ) : !data?.items.length ? (
          <div className="discovery-feedback glass">
            <h2>{c('emptyFilteredTitle')}</h2>
            <p>{c('emptyFilteredHint')}</p>
            <Button type="button" variant="outline" onClick={reset}>
              {c('resetFilters')}
            </Button>
          </div>
        ) : (
          data.items.map((set) => (
            <OfficialSetCard set={set} key={set.official_set_id} returnTo={returnTo} />
          ))
        )}
      </section>
      {!loading &&
        !error &&
        data &&
        (safeCursor || (data.cursor_string && data.cursor_string !== safeCursor)) && (
          <nav className="discovery-pagination" aria-label={c('pagination')}>
            {safeCursor && (
              <Button variant="outline" type="button" onClick={() => changePage(null)}>
                <ArrowLeft aria-hidden="true" />
                {c('firstPage')}
              </Button>
            )}
            {data.cursor_string && data.cursor_string !== safeCursor && (
              <Button
                variant="outline"
                type="button"
                onClick={() => changePage(data.cursor_string)}
              >
                {c('next')}
                <ArrowRight aria-hidden="true" />
              </Button>
            )}
          </nav>
        )}
    </div>
  );
}

export function OfficialBeatmapsetPage({ setId }: { setId: string }) {
  const t = useTranslations('publicOfficialMaps');
  const c = useTranslations('mapDiscovery');
  const maps = useTranslations('maps');
  const { user } = useSession();
  const params = useSearchParams();
  const valid =
    /^\d+$/.test(setId) &&
    Number.isSafeInteger(Number(setId)) &&
    Number(setId) > 0 &&
    Number(setId) < 2_000_000_000;
  const { data, error, loading, retry } = usePublicData<OfficialBeatmapset>(
    `/official/beatmapsets/${valid ? setId : '0'}`,
  );
  const returnTo = safeCatalogueReturn(params.get('returnTo')) ?? '/beatmaps';
  const selected =
    data?.difficulties.find((map) => String(map.official_id) === params.get('difficulty')) ??
    data?.difficulties[0];
  const stored = data?.difficulties.find(
    (map) => map.local_id !== undefined && map.archive_available,
  );
  const archive = data?.local.archive_available && stored?.local_id !== undefined;
  const download =
    data && !data.download_disabled
      ? archive
        ? `/api/v1/beatmaps/${stored!.local_id}/archive`
        : `https://osu.direct/api/d/${data.official_set_id}`
      : null;
  function choose(id: number) {
    const next = new URLSearchParams(window.location.search);
    next.set('difficulty', String(id));
    window.history.replaceState(null, '', `/beatmaps/official/${setId}?${next}`);
  }
  return (
    <div className="page-container map-discovery public-official-detail">
      <Link className="text-link back-link" href={returnTo}>
        <ArrowLeft aria-hidden="true" />
        {maps('back')}
      </Link>
      {loading ? (
        <div className="discovery-skeleton" role="status" aria-label={c('loading')}>
          <span />
          <span />
          <span />
        </div>
      ) : error || !data || !selected ? (
        <OfficialFeedback error={error} retry={retry} />
      ) : (
        <>
          <header className="public-official-header glass">
            <MapArtwork source="official" nativeSetId={data.official_set_id} priority />
            <div className="public-official-title">
              <OfficialStatus value={data.status} />
              <h1>{data.title}</h1>
              <p>{data.artist}</p>
              <span>{c('mappedBy', { creator: data.creator })}</span>
              <div className="actions">
                {download ? (
                  <Button asChild>
                    <a href={download} rel={archive ? undefined : 'noopener noreferrer'}>
                      <Download aria-hidden="true" />
                      {maps('downloadPackage')}
                    </a>
                  </Button>
                ) : (
                  <Button disabled>
                    <Download aria-hidden="true" />
                    {maps('downloadPackage')}
                  </Button>
                )}
                <Button asChild variant="outline">
                  <a href={data.official_url} target="_blank" rel="noopener noreferrer">
                    <ExternalLink aria-hidden="true" />
                    {maps('officialSetPage')}
                  </a>
                </Button>
              </div>
              <p className="hint">
                {data.download_disabled
                  ? maps('officialSearch.downloadDisabled')
                  : maps(archive ? 'archiveDownloadHint' : 'mirrorDownloadHint')}
              </p>
            </div>
          </header>
          <section
            className="public-official-difficulties glass"
            aria-labelledby="official-difficulties-title"
          >
            <div className="public-official-difficulty-heading">
              <h2 id="official-difficulties-title">{c('difficultyPreview')}</h2>
              <span>{c('fullPreviewCount', { count: data.difficulties.length })}</span>
            </div>
            <div className="public-official-difficulty-layout">
              <div
                className="public-official-difficulty-list"
                role="group"
                aria-label={c('difficultyPreview')}
              >
                {data.difficulties.map((map) => (
                  <button
                    key={map.official_id}
                    type="button"
                    className="difficulty-chip"
                    aria-pressed={selected.official_id === map.official_id}
                    onClick={() => choose(map.official_id)}
                    style={
                      {
                        '--difficulty-color': `var(--difficulty-${difficultyLevel(map.stars)})`,
                      } as CSSProperties
                    }
                  >
                    <span className="difficulty-marker" aria-hidden="true" />
                    <span className="difficulty-chip-copy">
                      <span className="difficulty-mode">{modes[map.mode]}</span>
                      <span className="difficulty-version">{map.version}</span>
                    </span>
                    <span className="difficulty-rating">
                      <Star aria-hidden="true" />
                      {map.stars.toFixed(2)}
                    </span>
                  </button>
                ))}
              </div>
              <div className="public-official-selected">
                <h3>{selected.version}</h3>
                <dl>
                  {[
                    [maps('stars'), selected.stars.toFixed(2)],
                    [maps('length'), mapDuration(selected.length_seconds)],
                    [maps('bpm'), selected.bpm?.toFixed(0) ?? '—'],
                    ['AR', selected.ar?.toFixed(1) ?? '—'],
                    ['CS', selected.cs?.toFixed(1) ?? '—'],
                    ['OD', selected.od?.toFixed(1) ?? '—'],
                    ['HP', selected.hp?.toFixed(1) ?? '—'],
                    [t('maxCombo'), selected.max_combo === null ? '—' : `${selected.max_combo}x`],
                  ].map(([label, value]) => (
                    <div key={label}>
                      <dt>{label}</dt>
                      <dd>{value}</dd>
                    </div>
                  ))}
                </dl>
                {selected.local_id !== undefined ? (
                  <Button asChild>
                    <Link
                      href={`/beatmaps/${selected.local_id}?returnTo=${encodeURIComponent(returnTo)}`}
                    >
                      <ArrowRight aria-hidden="true" />
                      {t('serverScores')}
                    </Link>
                  </Button>
                ) : (
                  <p className="hint">{t('metadataOnly')}</p>
                )}
                {user && hasPermission(user.permissions, 'beatmaps.import') && (
                  <Link className="text-link" href="/beatmaps/import">
                    {maps('import')}
                  </Link>
                )}
              </div>
            </div>
          </section>
        </>
      )}
    </div>
  );
}
