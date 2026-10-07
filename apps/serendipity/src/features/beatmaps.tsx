'use client';
import { useDeferredValue, useEffect, useRef, useState, type FormEvent } from 'react';
import Link from 'next/link';
import { useSearchParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import useSWR from 'swr';
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
  BeatmapDifficulty,
  BeatmapUploadResult,
  Page,
  Score,
  ScoreVariant,
} from '@/lib/types';
import { ScoreTable } from './scores';
import { ScoreVariantControl, useScoreSelection } from '@/components/score-selection';

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

function UploadResult({ result }: { result: BeatmapUploadResult }) {
  const t = useTranslations('maps');
  const [page, setPage] = useState(1);
  const archive = result.package.kind === 'osz';
  return (
    <section className="map-upload-result" aria-labelledby="map-upload-result-title">
      <h2 id="map-upload-result-title">{t('uploadResultTitle')}</h2>
      <p className="form-success" role="status">
        {t(result.idempotent ? 'uploadRetrySuccess' : 'uploadSuccess')}
      </p>
      <p className="hint">
        {t('packageCount', { count: result.items.length })} · {t('resultStatusHint')}
      </p>
      {result.idempotent && <p className="hint">{t('uploadRetryHint')}</p>}
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
  const [search, setSearch] = useState('');
  const q = useDeferredValue(search);
  const [mode, setMode] = useState('all');
  const [status, setStatus] = useState('all');
  const [page, setPage] = useState(1);
  const query = new URLSearchParams({ page: String(page), page_size: '24', q });
  if (mode !== 'all') query.set('mode', mode);
  if (status !== 'all') query.set('status', status);
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
        <GameMode
          value={mode}
          onChange={(v) => {
            setMode(v);
            setPage(1);
          }}
          all
        />
      </div>
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

export function BeatmapPage({ id }: { id: number }) {
  const t = useTranslations('maps');
  const c = useTranslations('common');
  const { data: map, error, isLoading, mutate } = useSWR<Beatmap>(`/beatmaps/${id}`, fetcher);
  const search = useSearchParams();
  const { mode: requestedMode, select } = useScoreSelection(map?.mode ?? 0);
  const mode = map && map.mode !== 0 ? String(map.mode) : requestedMode;
  const variant: ScoreVariant =
    mode !== '3' && search.get('variant') === 'relax' ? 'relax' : 'vanilla';
  const selection = new URLSearchParams({ mode, variant });
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
  } = useSWR<Page<Score>>(
    map ? `/beatmaps/${id}/scores?${selection}&page=${page}&page_size=24` : null,
    fetcher,
  );
  return (
    <div className="page-container">
      <Link href="/beatmaps" className="back-link">
        {t('back')}
      </Link>
      {isLoading ? (
        <Loading />
      ) : error ? (
        <LoadError retry={() => mutate()} />
      ) : (
        map && (
          <>
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
                </div>
              </div>
              <MapDownloads map={map} />
            </section>
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
            {map.package && (
              <PackageDifficulties
                key={`${map.id}:${map.package.id}`}
                mapId={map.id}
                currentVersion={map.version}
              />
            )}
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
                  <ScoreTable scores={scores.items} leaderboard />
                  <Pagination page={page} total={scores.total} onChange={setPage} />
                </>
              ) : (
                <Empty title={c('empty')} />
              )}
            </section>
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [errorCode, setErrorCode] = useState('');
  const [result, setResult] = useState<BeatmapUploadResult | null>(null);
  const [imported, setImported] = useState<Beatmap | null>(null);
  function clearFeedback() {
    setError('');
    setErrorCode('');
    setResult(null);
    setImported(null);
  }
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (submitting.current) return;
    submitting.current = true;
    const form = e.currentTarget;
    const data = new FormData(form);
    setBusy(true);
    clearFeedback();
    try {
      if (official)
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
      form.reset();
    } catch (cause) {
      if (cause instanceof ApiError && cause.status >= 400 && cause.status < 500) {
        const errorKeys = {
          official_beatmap_requires_import: 'errorOfficialUpload',
          map_package_conflict: 'errorPackageConflict',
          official_namespace_conflict: 'errorOfficialNamespace',
          official_version_conflict: 'errorOfficialVersion',
          unauthorized: 'errorUnauthorized',
          forbidden: 'errorForbidden',
        } as const;
        const key = errorKeys[cause.code as keyof typeof errorKeys];
        setError(key ? t(key) : cause.message);
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
        <form onSubmit={submit} className="glass upload-form">
          {official ? (
            <div className="field">
              <label htmlFor="official_id">{t('officialId')}</label>
              <Input
                id="official_id"
                name="official_id"
                type="number"
                min={1}
                max={1_999_999_999}
                step={1}
                required
                disabled={busy}
                onChange={clearFeedback}
              />
            </div>
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
              <p className="form-error" role="alert">
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
              {t(official ? 'import' : 'upload')}
            </Button>
            <Button variant="ghost" asChild>
              <Link href="/beatmaps">{c('back')}</Link>
            </Button>
          </div>
          {result && <UploadResult result={result} />}
          {imported && (
            <section className="map-upload-result" aria-labelledby="map-upload-result-title">
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
