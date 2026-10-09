'use client';

import { useEffect, useRef, useState, type CSSProperties, type FormEvent } from 'react';
import { useTranslations } from 'next-intl';
import { ChevronDown, Clock, ExternalLink, LoaderCircle, Music2, Search, Star } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldSelect } from '@/components/ui/field-select';
import { useNavigationLoading } from '@/components/navigation-motion';
import { api, ApiError } from '@/lib/api';
import { difficultyLevel } from './map-discovery';

type OfficialStatus =
  'ranked' | 'approved' | 'qualified' | 'loved' | 'pending' | 'wip' | 'graveyard';
export type OfficialSearchSet = {
  official_set_id: number;
  title: string;
  artist: string;
  creator: string;
  status: OfficialStatus;
  cover_url: string;
  official_url: string;
  download_disabled: boolean;
  difficulties: {
    official_id: number;
    version: string;
    mode: number;
    stars: number;
    length_seconds: number;
  }[];
  local: { stored_count: number; archive_available: boolean; withdrawn: boolean };
};
export type OfficialSearchResult = {
  items: OfficialSearchSet[];
  total: number;
  cursor_string: string | null;
};
type Filters = { q: string; mode: string; status: string; sort: string };
type Choose = (kind: 'difficulty' | 'archive', id: number) => void;
const defaultFilters: Filters = { q: '', mode: 'all', status: 'ranked', sort: 'ranked_desc' };
const modes = ['osu!', 'taiko', 'catch', 'mania'];

function duration(seconds: number) {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

function SearchSet({
  set,
  expanded,
  onExpand,
  onChoose,
  disabled,
}: {
  set: OfficialSearchSet;
  expanded: boolean;
  onExpand: () => void;
  onChoose: Choose;
  disabled: boolean;
}) {
  const t = useTranslations('maps.officialSearch');
  const maps = useTranslations('maps');
  const [coverFailed, setCoverFailed] = useState(false);
  const [difficultyId, setDifficultyId] = useState(String(set.difficulties[0]?.official_id ?? ''));
  const chosen =
    set.difficulties.find((item) => String(item.official_id) === difficultyId) ??
    set.difficulties[0];
  const canChoose = !disabled && !set.local.withdrawn;
  const detailId = `official-search-detail-${set.official_set_id}`;
  return (
    <li className="official-search-set" data-withdrawn={set.local.withdrawn}>
      <button
        type="button"
        className="official-search-summary"
        aria-expanded={expanded}
        aria-controls={detailId}
        onClick={onExpand}
      >
        <span className="official-search-cover" aria-hidden="true">
          {coverFailed || !set.cover_url ? (
            <Music2 />
          ) : (
            // The API bounds cover origins to osu! assets; no local upload or proxy is involved.
            // eslint-disable-next-line @next/next/no-img-element
            <img src={set.cover_url} alt="" loading="lazy" onError={() => setCoverFailed(true)} />
          )}
        </span>
        <span className="official-search-identity">
          <strong title={set.title}>{set.title}</strong>
          <span className="official-search-artist" title={set.artist}>
            {set.artist}
          </span>
          <span className="official-search-mapper" title={set.creator}>
            {maps('creator')} {set.creator}
          </span>
          <span className="official-search-facts">
            <span className="official-search-status" data-status={set.status}>
              {t(`statuses.${set.status}`)}
            </span>
            <span>{maps('packageCount', { count: set.difficulties.length })}</span>
            {set.local.withdrawn ? (
              <span className="official-search-withdrawn">{t('withdrawn')}</span>
            ) : set.local.archive_available ? (
              <span>{t('archiveStored')}</span>
            ) : set.local.stored_count > 0 ? (
              <span>{t('storedCount', { count: set.local.stored_count })}</span>
            ) : null}
          </span>
        </span>
        <ChevronDown className="official-search-chevron" aria-hidden="true" />
      </button>
      {expanded && (
        <div className="official-search-detail" id={detailId}>
          {set.local.withdrawn && <p className="hint">{t('withdrawnHint')}</p>}
          {set.download_disabled && <p className="hint">{t('downloadDisabled')}</p>}
          <div className="official-search-difficulty field">
            <label htmlFor={`official-search-difficulty-${set.official_set_id}`}>
              {maps('difficulty')}
            </label>
            {set.difficulties.length ? (
              <FieldSelect
                id={`official-search-difficulty-${set.official_set_id}`}
                value={chosen ? String(chosen.official_id) : ''}
                onValueChange={setDifficultyId}
                disabled={!canChoose}
                options={set.difficulties.map((item) => ({
                  value: String(item.official_id),
                  label: `${item.version} · ${modes[item.mode]} · ${item.stars.toFixed(2)} ${t('starsUnit')}`,
                }))}
              />
            ) : (
              <p className="hint">{t('noDifficulties')}</p>
            )}
            {chosen && (
              <p
                className="official-search-difficulty-facts"
                style={
                  {
                    '--difficulty-color': `var(--difficulty-${difficultyLevel(chosen.stars)})`,
                  } as CSSProperties
                }
              >
                <span>
                  <Star aria-hidden="true" />
                  {chosen.stars.toFixed(2)}
                </span>
                <span>
                  <Clock aria-hidden="true" />
                  {duration(chosen.length_seconds)}
                </span>
                <span>{modes[chosen.mode]}</span>
                <span>{t('difficultyId', { id: chosen.official_id })}</span>
              </p>
            )}
          </div>
          <div className="official-search-actions">
            <Button
              type="button"
              disabled={!canChoose || !chosen}
              onClick={() => chosen && onChoose('difficulty', chosen.official_id)}
            >
              {t('useDifficulty')}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={!canChoose}
              onClick={() => onChoose('archive', set.official_set_id)}
            >
              {t('useSet')}
            </Button>
            <a
              className="text-link"
              href={set.official_url}
              target="_blank"
              rel="noopener noreferrer"
            >
              {maps('officialSetPage')}
              <ExternalLink aria-hidden="true" />
            </a>
          </div>
          <p className="hint">{t('selectionHint')}</p>
        </div>
      )}
    </li>
  );
}

export function OfficialMapSearch({
  onChoose,
  disabled = false,
}: {
  onChoose: Choose;
  disabled?: boolean;
}) {
  const t = useTranslations('maps.officialSearch');
  const common = useTranslations('common');
  const [filters, setFilters] = useState<Filters>(defaultFilters);
  const [result, setResult] = useState<OfficialSearchResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [expanded, setExpanded] = useState<number | null>(null);
  const requestRef = useRef<AbortController | null>(null);
  const generation = useRef(0);
  const inFlight = useRef(false);
  const usedCursors = useRef(new Set<string>());
  const failedCursor = useRef<string | null>(null);
  useNavigationLoading(loading);

  useEffect(
    () => () => {
      generation.current += 1;
      requestRef.current?.abort();
    },
    [],
  );

  function changeFilters(next: Filters) {
    generation.current += 1;
    requestRef.current?.abort();
    requestRef.current = null;
    inFlight.current = false;
    usedCursors.current.clear();
    failedCursor.current = null;
    setFilters(next);
    setLoading(false);
    setResult(null);
    setError('');
    setExpanded(null);
  }

  async function search(cursor: string | null = null) {
    if (disabled || inFlight.current || (cursor && usedCursors.current.has(cursor))) return;
    inFlight.current = true;
    requestRef.current?.abort();
    const controller = new AbortController();
    requestRef.current = controller;
    const requestGeneration = ++generation.current;
    setLoading(true);
    setError('');
    failedCursor.current = cursor;
    if (!cursor) {
      setResult(null);
      setExpanded(null);
      usedCursors.current.clear();
    }
    const query = new URLSearchParams({ ...filters, q: filters.q.trim() });
    if (cursor) query.set('cursor_string', cursor);
    try {
      const response = await api<OfficialSearchResult>(`/beatmaps/official/search?${query}`, {
        signal: controller.signal,
      });
      if (controller.signal.aborted || generation.current !== requestGeneration) return;
      if (cursor) usedCursors.current.add(cursor);
      const nextCursor =
        response.cursor_string && !usedCursors.current.has(response.cursor_string)
          ? response.cursor_string
          : null;
      setResult((previous) => {
        const items = new Map<number, OfficialSearchSet>();
        for (const item of cursor ? (previous?.items ?? []) : [])
          items.set(item.official_set_id, item);
        for (const item of response.items) items.set(item.official_set_id, item);
        return { items: [...items.values()], total: response.total, cursor_string: nextCursor };
      });
    } catch (cause) {
      if (controller.signal.aborted || generation.current !== requestGeneration) return;
      const keys = {
        official_api_not_configured: 'errorConfig',
        official_auth_failed: 'errorAuth',
        official_api_failed: 'errorUnavailable',
        official_search_invalid: 'errorUnavailable',
        official_search_timeout: 'errorTimeout',
        official_rate_limited: 'errorRateLimit',
        official_resolution_busy: 'errorBusy',
        invalid_request: 'errorQuery',
        unauthorized: 'errorSignIn',
        forbidden: 'errorPermission',
      } as const;
      const key = cause instanceof ApiError ? keys[cause.code as keyof typeof keys] : undefined;
      setError(t(key ?? 'errorUnavailable'));
    } finally {
      if (generation.current === requestGeneration) {
        requestRef.current = null;
        inFlight.current = false;
        setLoading(false);
      }
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void search();
  }

  return (
    <section
      className="official-map-search glass"
      data-material="section"
      aria-labelledby="official-search-title"
    >
      <header className="official-search-heading">
        <h2 id="official-search-title">{t('title')}</h2>
        <p>{t('hint')}</p>
      </header>
      <form className="official-search-toolbar" onSubmit={submit} aria-busy={loading}>
        <div className="official-search-query official-search-field">
          <label htmlFor="official-search-query">{t('queryLabel')}</label>
          <div>
            <Input
              id="official-search-query"
              type="search"
              value={filters.q}
              maxLength={200}
              disabled={disabled}
              placeholder={t('queryPlaceholder')}
              onChange={(event) => changeFilters({ ...filters, q: event.target.value })}
            />
            <Button type="submit" disabled={disabled || loading}>
              {loading ? (
                <LoaderCircle className="spin" aria-hidden="true" />
              ) : (
                <Search aria-hidden="true" />
              )}
              {common('search')}
            </Button>
          </div>
        </div>
        <div className="official-search-field">
          <label htmlFor="official-search-mode">{t('mode')}</label>
          <FieldSelect
            id="official-search-mode"
            value={filters.mode}
            disabled={disabled}
            onValueChange={(mode) => changeFilters({ ...filters, mode })}
            options={[
              { value: 'all', label: t('allModes') },
              ...modes.map((mode, index) => ({ value: String(index), label: mode })),
            ]}
          />
        </div>
        <div className="official-search-field">
          <label htmlFor="official-search-status">{t('status')}</label>
          <FieldSelect
            id="official-search-status"
            value={filters.status}
            disabled={disabled}
            onValueChange={(status) => changeFilters({ ...filters, status })}
            options={(['any', 'ranked', 'qualified', 'loved', 'pending', 'graveyard'] as const).map(
              (status) => ({
                value: status,
                label: status === 'any' ? t('allStatuses') : t(`statuses.${status}`),
              }),
            )}
          />
        </div>
        <div className="official-search-field">
          <label htmlFor="official-search-sort">{t('sort')}</label>
          <FieldSelect
            id="official-search-sort"
            value={filters.sort}
            disabled={disabled}
            onValueChange={(sort) => changeFilters({ ...filters, sort })}
            options={(['ranked_desc', 'date_desc', 'title_asc', 'difficulty_desc'] as const).map(
              (sort) => ({ value: sort, label: t(`sorts.${sort}`) }),
            )}
          />
        </div>
      </form>
      <div className="official-search-feedback" role="status" aria-live="polite">
        {loading
          ? t(result ? 'loadingMore' : 'loading')
          : result
            ? t('resultCount', { shown: result.items.length, total: result.total })
            : !error
              ? t('ready')
              : ''}
      </div>
      {error && (
        <div className="official-search-error">
          <p className="form-error" role="alert">
            {error}
          </p>
          <Button
            type="button"
            variant="outline"
            disabled={disabled || loading}
            onClick={() => void search(failedCursor.current)}
          >
            {common('retry')}
          </Button>
        </div>
      )}
      {loading && !result && (
        <div className="official-search-skeleton" aria-hidden="true">
          {[0, 1, 2].map((item) => (
            <div key={item}>
              <span />
              <span>
                <i />
                <i />
                <i />
              </span>
            </div>
          ))}
        </div>
      )}
      {result &&
        (result.items.length ? (
          <>
            <ul className="official-search-results">
              {result.items.map((set) => (
                <SearchSet
                  key={set.official_set_id}
                  set={set}
                  expanded={expanded === set.official_set_id}
                  onExpand={() =>
                    setExpanded(expanded === set.official_set_id ? null : set.official_set_id)
                  }
                  onChoose={onChoose}
                  disabled={disabled}
                />
              ))}
            </ul>
            {result.cursor_string && (
              <div className="official-search-more">
                <Button
                  type="button"
                  variant="outline"
                  disabled={disabled || loading}
                  onClick={() => void search(result.cursor_string)}
                >
                  {loading ? <LoaderCircle className="spin" aria-hidden="true" /> : null}
                  {t('loadMore')}
                </Button>
              </div>
            )}
          </>
        ) : (
          <div className="official-search-empty">
            <strong>{t('emptyTitle')}</strong>
            <p>{t('emptyHint')}</p>
            <Button
              type="button"
              variant="ghost"
              disabled={disabled}
              onClick={() => {
                changeFilters(defaultFilters);
                document.getElementById('official-search-query')?.focus();
              }}
            >
              {t('resetFilters')}
            </Button>
          </div>
        ))}
    </section>
  );
}
