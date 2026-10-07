'use client';
import { useDeferredValue, useState, type FormEvent } from 'react';
import Link from 'next/link';
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
import { api, fetcher, hasPermission } from '@/lib/api';
import type { Beatmap, Page, Score } from '@/lib/types';
import { ScoreTable } from './scores';

function duration(seconds: number) {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
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
  const { data: scores, error: scoreError } = useSWR<Page<Score>>(
    map ? `/scores?beatmap_id=${id}` : null,
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
              <Button asChild>
                <a href={`/api/v1/beatmaps/${id}/download`}>
                  <Download />
                  {c('download')}
                </a>
              </Button>
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
            <section className="glass content-panel">
              <h2>{t('scores')}</h2>
              {scoreError ? (
                <LoadError />
              ) : !scores ? (
                <Loading />
              ) : scores.items.length ? (
                <ScoreTable scores={scores.items} />
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState(false);
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = e.currentTarget;
    const data = new FormData(form);
    setBusy(true);
    setError('');
    setSuccess(false);
    try {
      if (official)
        await api('/beatmaps/import', {
          method: 'POST',
          body: JSON.stringify({ official_id: Number(data.get('official_id')) }),
        });
      else await api('/beatmaps/upload', { method: 'POST', body: data });
      setSuccess(true);
      form.reset();
    } catch {
      setError(c('serviceError'));
    } finally {
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
              <Input id="official_id" name="official_id" type="number" min={1} required />
            </div>
          ) : (
            <div className="field">
              <label htmlFor="mapfile">{c('file')}</label>
              <label className="drop-zone">
                <Upload />
                <strong>{t('upload')}</strong>
                <span>{t('fileHint')}</span>
                <input type="file" name="file" id="mapfile" accept=".osu,.osz" required />
              </label>
            </div>
          )}
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          {success && (
            <p className="form-success" role="status">
              {t(official ? 'importSuccess' : 'uploadSuccess')}
            </p>
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
        </form>
      </Protected>
    </div>
  );
}
