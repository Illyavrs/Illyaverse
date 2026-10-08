'use client';
import { useDeferredValue, useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import Link from 'next/link';
import { useSearchParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import useSWR, { unstable_serialize, useSWRConfig } from 'swr';
import {
  ArrowRight,
  Search,
  Plus,
  Download,
  Music2,
  Star,
  Clock,
  Upload,
  LoaderCircle,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import {
  PageHeading,
  Loading,
  LoadError,
  Empty,
  GameMode,
  Pagination,
  Protected,
} from '@/components/common';
import { useSession } from '@/components/providers';
import { api, ApiError, fetcher, hasPermission } from '@/lib/api';
import type {
  Beatmap,
  BeatmapAvailability,
  BeatmapDifficulty,
  BeatmapUploadResult,
  Page,
  ScoreBoard,
  ScoreVariant,
} from '@/lib/types';
import { ScoreTable } from './scores';
import { ScoreVariantControl, useScoreSelection } from '@/components/score-selection';
import { BeatmapAvailabilityAction } from './admin';

function duration(seconds: number) {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
function DifficultyRows({ items, currentId }: { items: BeatmapDifficulty[]; currentId?: number }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  return (
    <div className="map-list package-difficulty-list">
      {items.map((map) => (
        <Link
          href={`/beatmaps/${map.id}`}
          key={map.id}
          className="map-row package-difficulty-row"
          aria-current={map.id === currentId ? 'page' : undefined}
        >
          <span className={`map-art mode-${map.mode}`} aria-hidden="true">
            <Music2 />
          </span>
          <div className="map-main">
            <strong>{map.version}</strong>
            <span>
              {map.title} <span className="dot">·</span> {map.artist}
            </span>
            <span>
              {t('creator')} {map.creator}
            </span>
            <div className="map-tags">
              <span className="package-mode">{['osu!', 'taiko', 'catch', 'mania'][map.mode]}</span>
              <span className={`status-text status-${map.status}`}>
                {c(
                  ['pending', 'ranked', 'loved', 'unranked'].includes(map.status)
                    ? map.status
                    : 'pending',
                )}
              </span>
              {map.id === currentId && <Badge variant="outline">{t('currentDifficulty')}</Badge>}
            </div>
          </div>
          <div className="map-numbers">
            <strong>
              <Star />
              {map.stars.toFixed(2)}
            </strong>
            <span>
              <Clock />
              {duration(map.length_seconds)}
            </span>
          </div>
          <ArrowRight className="row-arrow" />
        </Link>
      ))}
    </div>
  );
}

function MapDownloads({ map }: { map: Beatmap }) {
  const t = useTranslations('maps');
  const archive = map.download_kind === 'osz';
  const downloadUrl = map.download_url || `/api/v1/beatmaps/${map.id}/download`;
  return (
    <div className="map-downloads">
      <div className="actions">
        <Button asChild>
          <a href={archive ? (map.package?.download_url ?? downloadUrl) : downloadUrl}>
            <Download />
            {t(archive ? 'downloadPackage' : 'downloadDifficulty')}
          </a>
        </Button>
        {archive && (
          <Button asChild variant="outline">
            <a href={`/api/v1/beatmaps/${map.id}/osu`}>
              <Download />
              {t('downloadDifficulty')}
            </a>
          </Button>
        )}
      </div>
      <p className="hint">{t('singleFileHint')}</p>
    </div>
  );
}

function PackageDifficulties({ mapId, currentVersion }: { mapId: number; currentVersion: string }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  const [page, setPage] = useState(1);
  const { data, error, isLoading, mutate } = useSWR<Page<BeatmapDifficulty>>(
    `/beatmaps/${mapId}/package/difficulties?page=${page}&page_size=24`,
    fetcher,
    { keepPreviousData: false },
  );
  return (
    <section
      className="glass content-panel map-package"
      aria-labelledby="map-package-title"
      aria-busy={isLoading}
    >
      <header className="section-heading">
        <div>
          <h2 id="map-package-title">{t('packageTitle')}</h2>
          <p>{data ? t('packageCount', { count: data.total }) : c('loading')}</p>
          <p className="package-current">
            {t('currentDifficulty')}: <strong>{currentVersion}</strong>
          </p>
        </div>
      </header>
      {error ? (
        <LoadError retry={() => mutate()} />
      ) : !data ? (
        <Loading />
      ) : data.items.length ? (
        <DifficultyRows items={data.items} currentId={mapId} />
      ) : (
        <Empty title={c('empty')} />
      )}
      {data && !error && (
        <Pagination page={page} total={data.total} pageSize={data.page_size} onChange={setPage} />
      )}
    </section>
  );
}

function UploadResult({
  result,
  official = false,
}: {
  result: BeatmapUploadResult;
  official?: boolean;
}) {
  const t = useTranslations('maps');
  const [page, setPage] = useState(1);
  const archive = result.package.kind === 'osz';
  return (
    <section className="map-upload-result" aria-labelledby="map-upload-result-title" tabIndex={-1}>
      <h2 id="map-upload-result-title">
        {t(official ? 'importResultTitle' : 'uploadResultTitle')}
      </h2>
      <p className="form-success" role="status">
        {t(
          official
            ? result.idempotent
              ? 'importArchiveRetrySuccess'
              : 'importArchiveSuccess'
            : result.idempotent
              ? 'uploadRetrySuccess'
              : 'uploadSuccess',
        )}
      </p>
      <p className="hint">
        {t('packageCount', { count: result.items.length })} · {t('resultStatusHint')}
      </p>
      {result.idempotent && (
        <p className="hint">{t(official ? 'importArchiveRetryHint' : 'uploadRetryHint')}</p>
      )}
      <DifficultyRows items={result.items.slice((page - 1) * 24, page * 24)} />
      <Pagination page={page} total={result.items.length} onChange={setPage} />
      <div className="actions">
        <Button asChild variant="outline">
          <a href={result.package.download_url}>
            <Download />
            {t(archive ? 'downloadPackage' : 'downloadDifficulty')}
          </a>
        </Button>
      </div>
      {!archive && <p className="hint">{t('singleFileHint')}</p>}
    </section>
  );
}

export function MapRows({ items, compact = false }: { items: Beatmap[]; compact?: boolean }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  return (
    <div className={`map-list ${compact ? 'compact' : ''}`}>
      {items.map((map) => (
        <Link href={`/beatmaps/${map.id}`} key={map.id} className="map-row">
          <span className={`map-art mode-${map.mode}`} aria-hidden="true">
            <Music2 />
          </span>
          <div className="map-main">
            <strong>{map.title}</strong>
            <span>
              {map.artist} <span className="dot">·</span> {t('creator')} {map.creator}
            </span>
            <div className="map-tags">
              <Badge variant="secondary">{map.version}</Badge>
              <span className={`status-text status-${map.status}`}>
                {c(
                  ['pending', 'ranked', 'loved', 'unranked'].includes(map.status)
                    ? map.status
                    : 'pending',
                )}
              </span>
              {map.source === 'local' && <span className="exclusive">{c('local')}</span>}
            </div>
          </div>
          <div className="map-numbers">
            <strong>
              <Star />
              {map.stars.toFixed(2)}
            </strong>
            <span>
              <Clock />
              {duration(map.length_seconds)}
            </span>
          </div>
          <ArrowRight className="row-arrow" />
        </Link>
      ))}
    </div>
  );
}

export function BeatmapsPage() {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  const { user } = useSession();
  const params = useSearchParams();
  const uploader = params.get('uploaded_by');
  const [search, setSearch] = useState('');
  const q = useDeferredValue(search);
  const [mode, setMode] = useState('all');
  const [status, setStatus] = useState('all');
  const [source, setSource] = useState(() =>
    ['official', 'local'].includes(params.get('source') ?? '') ? params.get('source')! : 'all',
  );
  const [page, setPage] = useState(1);
  const query = new URLSearchParams({ page: String(page), page_size: '24', q });
  if (mode !== 'all') query.set('mode', mode);
  if (status !== 'all') query.set('status', status);
  if (source !== 'all') query.set('source', source);
  if (uploader !== null) query.set('uploaded_by', uploader);
  const { data, error, isLoading, mutate } = useSWR<Page<Beatmap>>(`/beatmaps?${query}`, fetcher);
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')}>
        {user && hasPermission(user.permissions, 'beatmaps.upload') && (
          <Button asChild>
            <Link href="/beatmaps/upload">
              <Plus />
              {t('upload')}
            </Link>
          </Button>
        )}
        {user && hasPermission(user.permissions, 'beatmaps.import') && (
          <Button asChild variant="outline">
            <Link href="/beatmaps/import">{t('import')}</Link>
          </Button>
        )}
      </PageHeading>
      <div className="filter-bar glass">
        <div className="search-input">
          <Search />
          <Input
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setPage(1);
            }}
            placeholder={t('search')}
            aria-label={t('search')}
          />
        </div>
        <select
          className="native-select"
          value={status}
          onChange={(e) => {
            setStatus(e.target.value);
            setPage(1);
          }}
          aria-label={c('status')}
        >
          {['all', 'ranked', 'loved', 'pending', 'unranked'].map((s) => (
            <option value={s} key={s}>
              {c(s)}
            </option>
          ))}
        </select>
        <select
          className="native-select"
          name="source"
          value={source}
          aria-label={t('source')}
          onChange={(event) => {
            setSource(event.target.value);
            setPage(1);
          }}
        >
          {['all', 'official', 'local'].map((value) => (
            <option value={value} key={value}>
              {c(value)}
            </option>
          ))}
        </select>
        <GameMode
          value={mode}
          onChange={(v) => {
            setMode(v);
            setPage(1);
          }}
          all
        />
      </div>
      {uploader !== null && (
        <div className="filter-context">
          <p>{t('uploaderFilter', { id: uploader })}</p>
          <Button
            variant="ghost"
            onClick={() => {
              const next = new URLSearchParams(window.location.search);
              next.delete('uploaded_by');
              window.history.replaceState(
                null,
                '',
                `${window.location.pathname}${next.size ? `?${next}` : ''}`,
              );
              setPage(1);
            }}
          >
            {t('clearUploader')}
          </Button>
        </div>
      )}
      <section className="glass content-panel">
        {isLoading ? (
          <Loading />
        ) : error ? (
          <LoadError retry={() => mutate()} />
        ) : !data?.items.length ? (
          <Empty title={t('emptyTitle')} description={t('emptyDescription')} />
        ) : (
          <MapRows items={data.items} />
        )}
      </section>
      {data && <Pagination page={page} total={data.total} onChange={setPage} />}
    </div>
  );
}

function boundedReviewSearch(title: string) {
  let value = '';
  let bytes = 0;
  const encoder = new TextEncoder();
  for (const character of title) {
    const length = encoder.encode(character).length;
    if (bytes + length > 100) break;
    value += character;
    bytes += length;
  }
  return value;
}

export function BeatmapPage({ id }: { id: number }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  const review = useTranslations('mapModeration');
  const availability = useTranslations('mapAvailability');
  const { user, loading: sessionLoading } = useSession();
  const search = useSearchParams();
  const management = search.get('management') === '1';
  const canManage = !sessionLoading && !!user && hasPermission(user.permissions, 'admin.view');
  const canReview = canManage && hasPermission(user.permissions, 'beatmaps.moderate');
  const { cache, mutate: mutateCache } = useSWRConfig();
  const mapKey = management
    ? canManage
      ? ([`/admin/beatmaps/${id}`, user.id] as const)
      : null
    : `/beatmaps/${id}`;
  const serializedKey = mapKey ? unstable_serialize(mapKey) : '';
  const [accessFailures, setAccessFailures] = useState<Record<string, ApiError>>({});
  const [acceptedAvailability, setAcceptedAvailability] = useState<
    Record<string, BeatmapAvailability>
  >({});
  const [availabilityStale, setAvailabilityStale] = useState(false);
  const {
    data: requestedMap,
    error: requestError,
    isLoading,
    mutate,
  } = useSWR<Beatmap>(
    mapKey,
    (key: string | readonly [string, number]) =>
      fetcher<Beatmap>(typeof key === 'string' ? key : key[0]),
    {
      keepPreviousData: false,
      dedupingInterval: 0,
      shouldRetryOnError: false,
      onError: (failure, requestedKey) => {
        if (!(failure instanceof ApiError) || ![401, 403, 404, 410].includes(failure.status))
          return;
        setAccessFailures((current) => ({ ...current, [requestedKey]: failure }));
        void mutateCache(requestedKey, undefined, { revalidate: false });
      },
      onSuccess: (_result, requestedKey) => {
        setAccessFailures((current) => {
          if (!current[requestedKey]) return current;
          const next = { ...current };
          delete next[requestedKey];
          return next;
        });
        setAcceptedAvailability((current) => {
          if (!current[requestedKey]) return current;
          const next = { ...current };
          delete next[requestedKey];
          return next;
        });
        if (requestedKey === serializedKey) setAvailabilityStale(false);
      },
    },
  );
  const error = requestError ?? accessFailures[serializedKey];
  const usableMap =
    !mapKey ||
    (management && !canManage) ||
    (error instanceof ApiError && [401, 403, 404, 410].includes(error.status))
      ? undefined
      : requestedMap;
  const map = useMemo(
    () =>
      usableMap && acceptedAvailability[serializedKey]
        ? { ...usableMap, availability: acceptedAvailability[serializedKey] }
        : usableMap,
    [usableMap, acceptedAvailability, serializedKey],
  );
  const withdrawn = map?.availability?.withdrawn ?? false;
  async function refreshAvailability() {
    try {
      const result = await mutate();
      const refreshed = !!result && !cache.get(serializedKey)?.error;
      setAvailabilityStale(!refreshed);
      return refreshed;
    } catch {
      setAvailabilityStale(true);
      return false;
    }
  }
  const { mode: requestedMode, select } = useScoreSelection(map?.mode ?? 0);
  const mode = map && map.mode !== 0 ? String(map.mode) : requestedMode;
  const variant: ScoreVariant =
    mode !== '3' && search.get('variant') === 'relax' ? 'relax' : 'vanilla';
  const selection = new URLSearchParams({ mode, variant });
  const requestedReturn = search.get('returnTo');
  let reviewReturn: string | null = null;
  if (canManage && requestedReturn) {
    try {
      const url = new URL(requestedReturn, 'https://illya.invalid');
      if (
        url.origin === 'https://illya.invalid' &&
        url.pathname === '/admin' &&
        url.searchParams.get('tab') === 'beatmaps'
      )
        reviewReturn = `${url.pathname}${url.search}`;
    } catch {
      /* Keep the normal catalogue link for malformed return targets. */
    }
  }
  useEffect(() => {
    if (!map) return;
    const invalidMode = search.has('mode') && search.get('mode') !== mode;
    const invalidVariant = search.has('variant') && search.get('variant') !== variant;
    if (!invalidMode && !invalidVariant) return;
    const query = new URLSearchParams(search.toString());
    query.set('mode', mode);
    query.set('variant', variant);
    query.delete('page');
    window.history.replaceState(null, '', `?${query}`);
  }, [map, mode, variant, search]);
  const [page, setPage] = useState(1);
  const {
    data: scores,
    error: scoreError,
    mutate: reloadScores,
  } = useSWR<ScoreBoard>(
    map && !withdrawn ? `/beatmaps/${id}/scores?${selection}&page=${page}&page_size=24` : null,
    fetcher,
  );
  return (
    <div className="page-container">
      <Link href={reviewReturn ?? '/beatmaps'} className="back-link">
        {reviewReturn ? review('backToQueue') : t('back')}
      </Link>
      {management && sessionLoading ? (
        <Loading />
      ) : management && !canManage ? (
        <Empty title={c(user ? 'noPermission' : 'signInRequired')} />
      ) : isLoading ? (
        <Loading />
      ) : error instanceof ApiError &&
        error.status === 410 &&
        error.code === 'beatmap_withdrawn' ? (
        <section className="glass content-panel map-withdrawn-public">
          <Empty title={availability('publicTitle')} description={availability('publicHint')} />
        </section>
      ) : error && !map ? (
        <LoadError retry={() => mutate()} />
      ) : (
        map && (
          <>
            {error && <LoadError retry={() => mutate()} />}
            <section className="map-detail-cover glass">
              <div className="map-detail-art" aria-hidden="true">
                <Music2 />
              </div>
              <div>
                <span className="map-artist">{map.artist}</span>
                <h1>{map.title}</h1>
                <p>
                  {map.version} <span className="dot">·</span> {t('creator')} {map.creator}
                </p>
                <div className="map-tags">
                  <Badge>{c(map.status)}</Badge>
                  <Badge variant="secondary">
                    {c(map.source === 'local' ? 'local' : 'official')}
                  </Badge>
                  {management && (
                    <Badge variant="outline">
                      {availability(withdrawn ? 'withdrawn' : 'active')}
                    </Badge>
                  )}
                </div>
              </div>
              <div className="map-detail-actions">
                {!withdrawn && <MapDownloads map={map} />}
                {management && canReview && (
                  <BeatmapAvailabilityAction
                    map={map}
                    disabled={availabilityStale}
                    changed={refreshAvailability}
                    accepted={(value) =>
                      setAcceptedAvailability((current) => ({ ...current, [serializedKey]: value }))
                    }
                  />
                )}
                {canManage && (
                  <Button asChild variant="outline">
                    <Link
                      href={
                        reviewReturn ??
                        `/admin?${new URLSearchParams({ tab: 'beatmaps', q: boundedReviewSearch(map.title), mode: String(map.mode), source: map.source, availability: withdrawn ? 'withdrawn' : 'active' })}`
                      }
                    >
                      {review('manageMap')}
                    </Link>
                  </Button>
                )}
              </div>
            </section>
            {management && (
              <section
                className="glass content-panel map-availability-summary"
                aria-label={availability('label')}
              >
                <h2>{availability(withdrawn ? 'withdrawn' : 'active')}</h2>
                <p className="hint">
                  {availability(withdrawn ? 'retainedHint' : 'managementHint')}
                </p>
                <p className="hint">
                  {(map.native_set_id ?? map.package?.native_set_id) !== undefined && (
                    <>
                      {availability('setId', {
                        id: (map.native_set_id ?? map.package?.native_set_id)!,
                      })}{' '}
                      ·{' '}
                    </>
                  )}
                  {availability('revision', { revision: map.availability?.revision ?? 0 })}
                </p>
                {map.availability?.reason && (
                  <p className="map-availability-reason">
                    <strong>{availability('lastReason')}</strong>
                    {map.availability.reason}
                  </p>
                )}
                {map.availability?.changed_at && (
                  <p className="hint">
                    {availability('changedAt')}:{' '}
                    <time dateTime={map.availability.changed_at}>
                      {map.availability.changed_at}
                    </time>
                  </p>
                )}
                {!canReview && <p className="hint">{availability('readOnly')}</p>}
                {availabilityStale && (
                  <div role="status">
                    <p className="hint">{availability('refreshFailed')}</p>
                    <Button type="button" variant="outline" onClick={refreshAvailability}>
                      {availability('refresh')}
                    </Button>
                  </div>
                )}
              </section>
            )}
            <dl className="map-detail-stats glass">
              {[
                [t('stars'), `${map.stars.toFixed(2)} ★`],
                [t('bpm'), map.bpm.toFixed(0)],
                [t('length'), duration(map.length_seconds)],
                [c('combo'), `${map.max_combo}x`],
              ].map(([label, value]) => (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd>{value}</dd>
                </div>
              ))}
            </dl>
            {map.package && !withdrawn && (
              <PackageDifficulties
                key={`${map.id}:${map.package.id}`}
                mapId={map.id}
                currentVersion={map.version}
              />
            )}
            {!withdrawn && (
              <section className="glass content-panel">
                <header className="section-heading">
                  <h2>{t('leaderboard')}</h2>
                </header>
                <div className="filter-bar">
                  <GameMode
                    value={mode}
                    allowedModes={map.mode === 0 ? undefined : [String(map.mode)]}
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
                </div>
                <p className="hint">
                  {mode === '3'
                    ? c('relaxUnsupported')
                    : t(variant === 'relax' ? 'relaxBoard' : 'normalBoard')}
                </p>
                {scoreError ? (
                  <LoadError retry={() => reloadScores()} />
                ) : !scores ? (
                  <Loading />
                ) : scores.items.length ? (
                  <>
                    <ScoreTable scores={scores.items} leaderboard orderedBy={scores.ordered_by} />
                    <Pagination page={page} total={scores.total} onChange={setPage} />
                  </>
                ) : (
                  <Empty title={c('empty')} />
                )}
              </section>
            )}
          </>
        )
      )}
    </div>
  );
}

export function MapUploadPage({ official = false }: { official?: boolean }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  const { user } = useSession();
  const submitting = useRef(false);
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLParagraphElement>(null);
  const [importKind, setImportKind] = useState<'difficulty' | 'archive'>('difficulty');
  const archiveImport = official && importKind === 'archive';
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [errorCode, setErrorCode] = useState('');
  const [result, setResult] = useState<BeatmapUploadResult | null>(null);
  const [imported, setImported] = useState<Beatmap | null>(null);
  const [feedbackRevision, setFeedbackRevision] = useState(0);
  useEffect(() => {
    if (error) errorRef.current?.focus();
    else if (result || imported)
      formRef.current?.querySelector<HTMLElement>('.map-upload-result')?.focus();
  }, [error, result, imported, feedbackRevision]);
  function clearFeedback() {
    setFeedbackRevision((revision) => revision + 1);
    setError('');
    setErrorCode('');
    setResult(null);
    setImported(null);
  }
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (submitting.current) return;
    const form = e.currentTarget;
    const data = new FormData(form);
    clearFeedback();
    if (archiveImport) {
      const setId = Number(data.get('official_set_id'));
      if (!Number.isInteger(setId) || setId < 1 || setId > 1_999_999_999) {
        setErrorCode('invalid_official_set_id');
        setError(t('errorOfficialSetId'));
        return;
      }
      const file = data.get('file');
      if (!(file instanceof File) || file.size === 0 || !/\.osz$/i.test(file.name)) {
        setErrorCode('invalid_archive_file');
        setError(t('errorArchiveFile'));
        return;
      }
      if (file.size > 64 * 1024 * 1024) {
        setErrorCode('archive_too_large');
        setError(t('errorArchiveTooLarge'));
        return;
      }
    }
    submitting.current = true;
    setBusy(true);
    try {
      if (archiveImport)
        setResult(
          await api<BeatmapUploadResult>('/beatmaps/import/archive', {
            method: 'POST',
            body: data,
          }),
        );
      else if (official)
        setImported(
          await api<Beatmap>('/beatmaps/import', {
            method: 'POST',
            body: JSON.stringify({ official_id: Number(data.get('official_id')) }),
          }),
        );
      else
        setResult(
          await api<BeatmapUploadResult>('/beatmaps/upload', { method: 'POST', body: data }),
        );
      if (official) {
        // Keep the inactive flow's draft, including its selected file.
        for (const name of archiveImport ? ['official_set_id', 'file'] : ['official_id']) {
          const input = form.elements.namedItem(name);
          if (input instanceof HTMLInputElement) input.value = '';
        }
      } else form.reset();
    } catch (cause) {
      if (cause instanceof ApiError) {
        const errorKeys = {
          official_beatmap_requires_import: 'errorOfficialUpload',
          map_package_conflict: 'errorPackageConflict',
          official_namespace_conflict: 'errorOfficialNamespace',
          official_version_conflict: 'errorOfficialVersion',
          official_set_version_conflict: 'errorOfficialSetVersion',
          official_archive_conflict: 'errorOfficialArchiveConflict',
          official_archive_manifest_mismatch: 'errorOfficialArchiveManifest',
          official_archive_invalid: 'errorOfficialArchiveInvalid',
          official_archive_too_large: 'errorArchiveTooLarge',
          official_identity_mismatch: 'errorOfficialIdentity',
          checksum_mismatch: 'errorOfficialChecksum',
          official_api_not_configured: 'errorOfficialAPIConfig',
          official_auth_failed: 'errorOfficialAPIAuth',
          official_api_failed: 'errorOfficialAPIFailed',
          official_rate_limited: 'errorOfficialRateLimit',
          beatmap_not_found: 'errorOfficialNotFound',
          beatmapset_not_found: 'errorOfficialNotFound',
          unauthorized: 'errorUnauthorized',
          forbidden: 'errorForbidden',
        } as const;
        const key = errorKeys[cause.code as keyof typeof errorKeys];
        setError(
          key
            ? t(key)
            : cause.status >= 400 && cause.status < 500
              ? cause.message
              : c('serviceError'),
        );
        setErrorCode(cause.code);
      } else setError(c('serviceError'));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  }
  return (
    <div className="page-container narrow">
      <PageHeading
        title={t(official ? 'importTitle' : 'uploadTitle')}
        description={t(official ? 'importDescription' : 'uploadDescription')}
      />
      <Protected permission={official ? 'beatmaps.import' : 'beatmaps.upload'}>
        <form ref={formRef} onSubmit={submit} className="glass upload-form" aria-busy={busy}>
          {official ? (
            <>
              <div className="field">
                <label htmlFor="official-import-kind">{t('importKind')}</label>
                <select
                  id="official-import-kind"
                  className="native-select"
                  value={importKind}
                  disabled={busy}
                  onChange={(e) => {
                    setImportKind(e.target.value as 'difficulty' | 'archive');
                    clearFeedback();
                  }}
                >
                  <option value="difficulty">{t('importDifficultyMode')}</option>
                  <option value="archive">{t('importArchiveMode')}</option>
                </select>
              </div>
              <div hidden={archiveImport}>
                <div className="field">
                  <label htmlFor="official_id">{t('officialId')}</label>
                  <Input
                    id="official_id"
                    name="official_id"
                    type="number"
                    min={1}
                    max={1_999_999_999}
                    step={1}
                    required={!archiveImport}
                    disabled={busy || archiveImport}
                    onChange={clearFeedback}
                  />
                  <p className="hint">{t('singleFileHint')}</p>
                </div>
              </div>
              <div hidden={!archiveImport}>
                <div className="field">
                  <label htmlFor="official_set_id">{t('officialSetId')}</label>
                  <Input
                    id="official_set_id"
                    name="official_set_id"
                    type="number"
                    min={1}
                    max={1_999_999_999}
                    step={1}
                    required={archiveImport}
                    disabled={busy || !archiveImport}
                    aria-describedby="official-set-hint"
                    aria-invalid={errorCode === 'invalid_official_set_id' || undefined}
                    onChange={clearFeedback}
                  />
                  <p className="hint" id="official-set-hint">
                    {t('officialSetIdHint')}
                  </p>
                </div>
                <div className="field">
                  <label htmlFor="official_archive_file">{t('archiveFile')}</label>
                  <label className="drop-zone">
                    <Upload />
                    <strong>{t('importArchiveAction')}</strong>
                    <span>{t('archiveFileHint')}</span>
                    <input
                      type="file"
                      name="file"
                      id="official_archive_file"
                      accept=".osz"
                      required={archiveImport}
                      disabled={busy || !archiveImport}
                      aria-describedby="official-archive-hint"
                      aria-invalid={
                        ['invalid_archive_file', 'archive_too_large'].includes(errorCode) ||
                        undefined
                      }
                      onChange={clearFeedback}
                    />
                  </label>
                  <p className="hint" id="official-archive-hint">
                    {t('archiveImportHint')}
                  </p>
                  <Link href="/docs/official-archives" className="text-link">
                    {t('archiveImportGuide')}
                    <ArrowRight />
                  </Link>
                </div>
              </div>
            </>
          ) : (
            <div className="field">
              <label htmlFor="mapfile">{c('file')}</label>
              <label className="drop-zone">
                <Upload />
                <strong>{t('upload')}</strong>
                <span>{t('fileHint')}</span>
                <input
                  type="file"
                  name="file"
                  id="mapfile"
                  accept=".osu,.osz"
                  required
                  disabled={busy}
                  aria-describedby="map-upload-hint"
                  onChange={clearFeedback}
                />
              </label>
              <p className="hint" id="map-upload-hint">
                {t('uploadPackageHint')}
              </p>
            </div>
          )}
          {error && (
            <div className="map-upload-error">
              <p className="form-error" role="alert" ref={errorRef} tabIndex={-1}>
                {error}
              </p>
              {errorCode === 'official_beatmap_requires_import' &&
                (user && hasPermission(user.permissions, 'beatmaps.import') ? (
                  <Link href="/beatmaps/import" className="text-link">
                    {t('import')}
                    <ArrowRight />
                  </Link>
                ) : (
                  <p className="hint">{t('officialImportPermissionHint')}</p>
                ))}
              {errorCode === 'unauthorized' && (
                <Link href="/login" className="text-link">
                  {c('signInRequired')}
                  <ArrowRight />
                </Link>
              )}
            </div>
          )}
          <div className="actions">
            <Button disabled={busy} type="submit">
              {busy ? <LoaderCircle className="spin" /> : <Upload />}
              {t(
                official
                  ? archiveImport
                    ? 'importArchiveAction'
                    : 'importDifficultyAction'
                  : 'upload',
              )}
            </Button>
            <Button variant="ghost" asChild>
              <Link href="/beatmaps">{c('back')}</Link>
            </Button>
          </div>
          {result && <UploadResult result={result} official={official} />}
          {imported && (
            <section
              className="map-upload-result"
              aria-labelledby="map-upload-result-title"
              tabIndex={-1}
            >
              <h2 id="map-upload-result-title">{t('importResultTitle')}</h2>
              <p className="form-success" role="status">
                {t('importSuccess')}
              </p>
              <p className="hint">{t('resultStatusHint')}</p>
              <DifficultyRows items={[imported]} />
            </section>
          )}
        </form>
      </Protected>
    </div>
  );
}
