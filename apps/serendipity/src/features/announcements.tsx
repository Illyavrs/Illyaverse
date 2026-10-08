'use client';

import { Suspense, useCallback, useEffect, useEffectEvent, useRef, useState } from 'react';
import Link from 'next/link';
import { useRouter, useSearchParams } from 'next/navigation';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR, { SWRConfig, useSWRConfig } from 'swr';
import { ArrowLeft, ArrowRight, LoaderCircle, Plus, RefreshCw } from 'lucide-react';
import { Avatar, Empty, Loading, PageHeading, Pagination } from '@/components/common';
import { useSession } from '@/components/providers';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldSelect } from '@/components/ui/field-select';
import { Textarea } from '@/components/ui/textarea';
import { api, ApiError, fetcher, hasPermission } from '@/lib/api';
import type { Announcement, AnnouncementStatus, Page } from '@/lib/types';
import { useAnnouncementDraft } from './announcement-draft';

const statuses: AnnouncementStatus[] = ['draft', 'published', 'archived'];
const isAccessError = (error: unknown) =>
  error instanceof ApiError && [401, 403].includes(error.status);
const privateAnnouncementKey = (key: unknown) =>
  typeof key === 'string' && key.startsWith('/admin/announcements');

function useAnnouncementError() {
  const t = useTranslations('announcements');
  const c = useTranslations('common');
  return (error: unknown) => {
    if (error instanceof ApiError) {
      if (error.status === 401) return t('unauthorized');
      if (error.status === 403) return t('forbidden');
      if (error.status === 404) return t('notFound');
      if (error.status === 409) return t('conflict');
      if (error.status === 400 || error.status === 422) return t('invalid');
    }
    return c('serviceError');
  };
}

function AnnouncementError({ error, retry }: { error: unknown; retry?: () => void }) {
  const describeError = useAnnouncementError();
  const c = useTranslations('common');
  return (
    <div className="announcement-load-error" role="alert">
      <p>{describeError(error)}</p>
      {retry && (
        <Button type="button" variant="outline" size="sm" onClick={retry}>
          <RefreshCw aria-hidden="true" />
          {c('retry')}
        </Button>
      )}
    </div>
  );
}

function AnnouncementMeta({
  announcement,
  updated = false,
}: {
  announcement: Announcement;
  updated?: boolean;
}) {
  const t = useTranslations('announcements');
  const f = useFormatter();
  const date = updated
    ? announcement.updated_at
    : (announcement.published_at ?? announcement.created_at);
  return (
    <div className="announcement-meta">
      <Link href={`/players/${announcement.author.id}`} className="announcement-author">
        <Avatar name={announcement.author.username} url={announcement.author.avatar_url} />
        <span>{announcement.author.username}</span>
      </Link>
      <time dateTime={date}>
        {updated && `${t('updated')} · `}
        {f.dateTime(new Date(date), { dateStyle: 'medium' })}
      </time>
    </div>
  );
}

function AnnouncementRows({
  items,
  compact = false,
}: {
  items: Announcement[];
  compact?: boolean;
}) {
  return (
    <ul className={`announcement-rows${compact ? ' announcement-rows-compact' : ''}`}>
      {items
        .filter((item) => item.status === 'published')
        .map((announcement) => (
          <li key={announcement.id}>
            <h2>
              <Link href={`/news/${announcement.id}`}>
                {announcement.title}
                <ArrowRight aria-hidden="true" />
              </Link>
            </h2>
            <AnnouncementMeta announcement={announcement} />
            {!compact && (
              <p className="announcement-excerpt">
                {announcement.body.replace(/\s+/g, ' ').slice(0, 200)}
                {announcement.body.length > 200 ? '…' : ''}
              </p>
            )}
          </li>
        ))}
    </ul>
  );
}

export function AnnouncementsPreview() {
  const t = useTranslations('announcements');
  const c = useTranslations('common');
  const { data, error, isLoading, mutate } = useSWR<Page<Announcement>>(
    '/announcements?page=1&page_size=3',
    fetcher,
    { shouldRetryOnError: false },
  );
  const items = data?.items.filter((item) => item.status === 'published').slice(0, 3) ?? [];
  return (
    <section
      className="announcements-preview"
      aria-labelledby="announcements-preview-title"
      aria-busy={isLoading}
    >
      <div className="home-section-heading">
        <h2 id="announcements-preview-title">{t('title')}</h2>
        <Link href="/news" className="text-link">
          {c('viewAll')}
          <ArrowRight aria-hidden="true" />
        </Link>
      </div>
      <div className="glass announcement-preview-content">
        {isLoading ? (
          <Loading />
        ) : error ? (
          <AnnouncementError error={error} retry={() => mutate()} />
        ) : items.length ? (
          <AnnouncementRows items={items} compact />
        ) : (
          <p className="announcement-empty">{t('empty')}</p>
        )}
      </div>
    </section>
  );
}

function readPage(query: Pick<URLSearchParams, 'get'>) {
  const raw = query.get('page') ?? '1';
  const value = /^\d+$/.test(raw) ? Number(raw) : 1;
  return Number.isSafeInteger(value) && value > 0 && value <= 4294967295 ? value : 1;
}

function NewsIndex() {
  const t = useTranslations('announcements');
  const query = useSearchParams();
  const page = readPage(query);
  const { data, error, isLoading, mutate } = useSWR<Page<Announcement>>(
    `/announcements?page=${page}&page_size=12`,
    fetcher,
    {
      keepPreviousData: false,
      shouldRetryOnError: false,
      onSuccess: (result, requestedKey) => {
        if (requestedKey !== `/announcements?page=${page}&page_size=12` || result.page_size < 1)
          return;
        const current = new URLSearchParams(window.location.search);
        if (readPage(current) !== result.page) return;
        const lastPage = Math.max(1, Math.ceil(result.total / result.page_size));
        if (result.page <= lastPage) return;
        if (lastPage === 1) current.delete('page');
        else current.set('page', String(lastPage));
        window.history.replaceState(null, '', `/news${current.size ? `?${current}` : ''}`);
      },
    },
  );
  const items = data?.items.filter((item) => item.status === 'published') ?? [];
  function selectPage(value: number) {
    const next = new URLSearchParams(query.toString());
    if (value === 1) next.delete('page');
    else next.set('page', String(value));
    window.history.pushState(null, '', `/news${next.size ? `?${next}` : ''}`);
  }
  return (
    <div className="page-container announcements-page">
      <PageHeading title={t('title')} description={t('description')} />
      <section className="glass announcement-index" aria-label={t('title')} aria-busy={isLoading}>
        {isLoading ? (
          <Loading />
        ) : error ? (
          <AnnouncementError error={error} retry={() => mutate()} />
        ) : items.length ? (
          <AnnouncementRows items={items} />
        ) : (
          <Empty title={t('empty')} description={t('emptyDescription')} />
        )}
      </section>
      {data && !error && (
        <Pagination
          page={page}
          total={data.total}
          pageSize={data.page_size}
          onChange={selectPage}
        />
      )}
    </div>
  );
}

export function AnnouncementsPage() {
  return (
    <Suspense fallback={<Loading />}>
      <NewsIndex />
    </Suspense>
  );
}

export function AnnouncementPage({ id }: { id: number }) {
  const t = useTranslations('announcements');
  const { data, error, isLoading, mutate } = useSWR<Announcement>(`/announcements/${id}`, fetcher, {
    shouldRetryOnError: false,
  });
  const missing = error instanceof ApiError && error.status === 404;
  const announcement = data?.status === 'published' && !error ? data : undefined;
  return (
    <div className="page-container announcement-detail">
      <Link href="/news" className="text-link announcement-back">
        <ArrowLeft aria-hidden="true" />
        {t('back')}
      </Link>
      {isLoading ? (
        <Loading />
      ) : error ? (
        <AnnouncementError error={error} retry={missing ? undefined : () => mutate()} />
      ) : !announcement ? (
        <Empty title={t('notFound')} />
      ) : (
        <article className="glass announcement-article">
          <header>
            <h1>{announcement.title}</h1>
            <AnnouncementMeta announcement={announcement} />
          </header>
          <div className="announcement-body">{announcement.body}</div>
        </article>
      )}
    </div>
  );
}

export type AnnouncementNavigationGuard = (action: () => void) => void;
type AdminAnnouncementProps = {
  registerNavigationGuard?: (guard: AnnouncementNavigationGuard | null) => void;
};

// This cache belongs to the visible permission boundary. Unmounting the tab or
// losing the session discards every private list instead of retaining hidden data.
export function AdminAnnouncements(props: AdminAnnouncementProps) {
  const { user } = useSession();
  const t = useTranslations('announcements');
  if (!user || !hasPermission(user.permissions, 'announcements.manage'))
    return <Empty title={t('forbidden')} />;
  return (
    <SWRConfig key={user.id} value={{ provider: () => new Map() }}>
      <AnnouncementWorkbench {...props} />
    </SWRConfig>
  );
}

function AnnouncementWorkbench({ registerNavigationGuard }: AdminAnnouncementProps) {
  const t = useTranslations('announcements');
  const c = useTranslations('common');
  const describeError = useAnnouncementError();
  const router = useRouter();
  const { mutate: clearPrivateCache } = useSWRConfig();
  const { refresh } = useSession();
  const draftScope = useAnnouncementDraft();
  const [recoveryDraft, setRecoveryDraft] = useState(() => draftScope?.read() ?? null);
  const [recovered, setRecovered] = useState(Boolean(recoveryDraft));
  const [recoveryStatus, setRecoveryStatus] = useState<'checking' | 'failed' | 'ready'>(
    recoveryDraft ? 'checking' : 'ready',
  );
  const [recoveryError, setRecoveryError] = useState<unknown>(null);
  const [recoveryAttempt, setRecoveryAttempt] = useState(0);
  const query = useSearchParams();
  const requestedStatus = query.get('status') ?? 'all';
  const status = statuses.includes(requestedStatus as AnnouncementStatus) ? requestedStatus : 'all';
  const page = readPage(query);
  const [accessLost, setAccessLost] = useState<unknown>(null);
  const [selected, setSelected] = useState<Announcement | null>(
    recoveryDraft?.announcement ?? null,
  );
  const [editing, setEditing] = useState(Boolean(recoveryDraft));
  const [title, setTitle] = useState(recoveryDraft?.title ?? '');
  const [body, setBody] = useState(recoveryDraft?.body ?? '');
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const mounted = useRef(true);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [conflict, setConflict] = useState(recoveryDraft?.conflict ?? false);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const [pending, setPending] = useState<{ action: () => void; reload?: boolean } | null>(null);
  const [withdrawConfirm, setWithdrawConfirm] = useState(false);
  const titleRef = useRef<HTMLInputElement>(null);
  const key = `/admin/announcements?page=${page}&page_size=12&status=${status}`;
  const {
    data,
    error: loadError,
    isLoading,
    mutate,
  } = useSWR<Page<Announcement>>(accessLost ? null : key, fetcher, {
    keepPreviousData: false,
    dedupingInterval: 0,
    shouldRetryOnError: false,
    onError: (failure) => {
      if (isAccessError(failure)) loseAccess(failure);
    },
    onSuccess: (result, requestedKey) => {
      if (requestedKey !== key || result.page_size < 1) return;
      const current = new URLSearchParams(window.location.search);
      if (
        current.get('tab') !== 'announcements' ||
        readPage(current) !== result.page ||
        (current.get('status') ?? 'all') !== status
      )
        return;
      const lastPage = Math.max(1, Math.ceil(result.total / result.page_size));
      if (result.page <= lastPage) return;
      if (lastPage === 1) current.delete('page');
      else current.set('page', String(lastPage));
      window.history.replaceState(null, '', `?${current}`);
    },
  });
  const dirty = editing && (title !== (selected?.title ?? '') || body !== (selected?.body ?? ''));
  const guard = useCallback<AnnouncementNavigationGuard>(
    (action) => {
      if (saving.current) return;
      if (dirty || recovered) setPending({ action });
      else action();
    },
    [dirty, recovered],
  );

  useEffect(() => {
    if (!draftScope) return;
    if (editing && !accessLost && (dirty || conflict || recovered))
      draftScope.retain({ announcement: selected, title, body, conflict });
    else draftScope.discard();
  }, [draftScope, editing, accessLost, dirty, conflict, recovered, selected, title, body]);

  const finishRecovery = useEffectEvent((latest: Announcement | null, failure?: unknown) => {
    if (failure) {
      if (isAccessError(failure)) {
        loseAccess(failure);
        return;
      }
      if (recoveryDraft?.announcement && failure instanceof ApiError && failure.status === 404) {
        setConflict(true);
        setError(describeError(failure));
        setRecoveryStatus('ready');
      } else {
        setRecoveryError(failure);
        setRecoveryStatus('failed');
      }
      return;
    }
    if (
      latest &&
      recoveryDraft?.announcement &&
      latest.revision !== recoveryDraft.announcement.revision
    ) {
      setConflict(true);
      setError(t('conflict'));
    } else {
      if (latest) setSelected(latest);
      if (recoveryDraft?.conflict) setError(t('conflict'));
    }
    setRecoveryStatus('ready');
  });
  useEffect(() => {
    if (!recoveryDraft || !recovered) return;
    let active = true;
    // Check current authority before revealing recovered private text. An absent
    // record still leaves its local text available for copying and explicit discard.
    const recoveryKey = recoveryDraft.announcement
      ? `/admin/announcements/${recoveryDraft.announcement.id}`
      : '/admin/announcements?page=1&page_size=1&status=all';
    api<Announcement | Page<Announcement>>(recoveryKey)
      .then((result) => {
        if (active) finishRecovery('revision' in result ? result : null);
      })
      .catch((failure) => {
        if (active) finishRecovery(null, failure);
      });
    return () => {
      active = false;
    };
  }, [recoveryDraft, recoveryAttempt, recovered]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    registerNavigationGuard?.(guard);
    return () => registerNavigationGuard?.(null);
  }, [guard, registerNavigationGuard]);
  useEffect(() => {
    if (!dirty && !recovered) return;
    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
    };
    const navigate = (event: MouseEvent) => {
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      )
        return;
      const link = (event.target as Element | null)?.closest<HTMLAnchorElement>('a[href]');
      if (!link || link.target === '_blank' || link.hasAttribute('download')) return;
      const target = new URL(link.href, window.location.href);
      if (
        target.href === window.location.href ||
        (target.pathname === window.location.pathname && target.search === window.location.search)
      )
        return;
      event.preventDefault();
      event.stopPropagation();
      guard(() => {
        if (target.origin === window.location.origin)
          router.push(`${target.pathname}${target.search}${target.hash}`);
        else window.location.assign(target.href);
      });
    };
    window.addEventListener('beforeunload', beforeUnload);
    document.addEventListener('click', navigate, true);
    return () => {
      window.removeEventListener('beforeunload', beforeUnload);
      document.removeEventListener('click', navigate, true);
    };
  }, [dirty, recovered, guard, router]);

  function loseAccess(failure: unknown) {
    setAccessLost(failure);
    setSelected(null);
    setTitle('');
    setBody('');
    setEditing(false);
    setPending(null);
    setConflict(false);
    setRecovered(false);
    setRecoveryDraft(null);
    draftScope?.discard();
    void clearPrivateCache(privateAnnouncementKey, undefined, { revalidate: false });
    void refresh();
  }
  function open(announcement: Announcement | null) {
    draftScope?.discard();
    setRecoveryDraft(null);
    setSelected(announcement);
    setTitle(announcement?.title ?? '');
    setBody(announcement?.body ?? '');
    setEditing(true);
    setError('');
    setSuccess('');
    setConflict(false);
    setWithdrawConfirm(false);
    setPending(null);
    setRecovered(false);
    setRecoveryStatus('ready');
    queueMicrotask(() => titleRef.current?.focus());
  }
  function close() {
    draftScope?.discard();
    setRecoveryDraft(null);
    setEditing(false);
    setSelected(null);
    setTitle('');
    setBody('');
    setError('');
    setConflict(false);
    setWithdrawConfirm(false);
    setRecovered(false);
    setRecoveryStatus('ready');
  }
  function select(filters: { page?: number; status?: string }) {
    if (saving.current) return;
    const next = new URLSearchParams(query.toString());
    next.set('tab', 'announcements');
    next.set('status', filters.status ?? status);
    const nextPage = filters.page ?? 1;
    if (nextPage > 1) next.set('page', String(nextPage));
    else next.delete('page');
    window.history.pushState(null, '', `?${next}`);
  }
  async function refreshList() {
    try {
      // Fetch directly: failed SWR revalidation can resolve with stale records.
      const result = await api<Page<Announcement>>(key);
      if (mounted.current) {
        await mutate(result, { revalidate: false });
        setRefreshFailed(false);
        const current = new URLSearchParams(window.location.search);
        if (
          current.get('tab') === 'announcements' &&
          readPage(current) === result.page &&
          (current.get('status') ?? 'all') === status &&
          result.page_size > 0
        ) {
          const lastPage = Math.max(1, Math.ceil(result.total / result.page_size));
          if (result.page > lastPage) {
            if (lastPage === 1) current.delete('page');
            else current.set('page', String(lastPage));
            window.history.replaceState(null, '', `?${current}`);
          }
        }
      }
    } catch (failure) {
      if (isAccessError(failure)) loseAccess(failure);
      else if (mounted.current) setRefreshFailed(true);
    }
  }
  async function save(nextStatus: AnnouncementStatus) {
    if (saving.current || conflict || accessLost || recoveryStatus !== 'ready') return;
    const normalizedTitle = title.trim();
    const normalizedBody = body.replace(/\r\n/g, '\n').trim();
    const chars = (value: string) => Array.from(value).length;
    const controls = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/;
    if (
      !normalizedTitle ||
      chars(normalizedTitle) > 160 ||
      !normalizedBody.trim() ||
      chars(normalizedBody) > 20000 ||
      new TextEncoder().encode(normalizedBody).length > 65536 ||
      /[\u0000-\u001f\u007f-\u009f]/.test(title) ||
      controls.test(normalizedBody)
    ) {
      setError(t('invalid'));
      return;
    }
    saving.current = true;
    setBusy(true);
    setError('');
    setSuccess('');
    try {
      let saved = selected;
      if (!saved) {
        saved = await api<Announcement>('/admin/announcements', {
          method: 'POST',
          body: JSON.stringify({ title: normalizedTitle, body: normalizedBody }),
        });
        if (!mounted.current) return;
        setSelected(saved);
        setTitle(saved.title);
        setBody(saved.body);
      }
      if (selected || nextStatus !== 'draft') {
        saved = await api<Announcement>(`/admin/announcements/${saved.id}`, {
          method: 'PATCH',
          body: JSON.stringify({
            title: normalizedTitle,
            body: normalizedBody,
            status: nextStatus,
            revision: saved.revision,
          }),
        });
      }
      if (!mounted.current) return;
      setSelected(saved);
      setTitle(saved.title);
      setBody(saved.body);
      setRecovered(false);
      setRecoveryDraft(null);
      draftScope?.discard();
      setWithdrawConfirm(false);
      setSuccess(
        t(
          nextStatus === 'published'
            ? 'publishedSuccess'
            : nextStatus === 'archived'
              ? 'withdrawnSuccess'
              : 'draftSuccess',
        ),
      );
      await refreshList();
    } catch (failure) {
      if (!mounted.current) return;
      if (isAccessError(failure)) loseAccess(failure);
      else {
        setError(describeError(failure));
        if (failure instanceof ApiError && failure.status === 409) setConflict(true);
      }
    } finally {
      saving.current = false;
      if (mounted.current) setBusy(false);
    }
  }
  async function reloadLatest() {
    if (!selected || saving.current) return;
    saving.current = true;
    setBusy(true);
    setError('');
    try {
      const latest = await api<Announcement>(`/admin/announcements/${selected.id}`);
      if (!mounted.current) return;
      open(latest);
      setSuccess(t('latestLoaded'));
      await refreshList();
    } catch (failure) {
      if (isAccessError(failure)) loseAccess(failure);
      else if (mounted.current) setError(describeError(failure));
    } finally {
      saving.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  if (accessLost) return <AnnouncementError error={accessLost} />;
  return (
    <div className="announcement-workbench" aria-busy={busy}>
      <div className="section-heading">
        <div>
          <h2>{t('manageTitle')}</h2>
          <p>{t('manageDescription')}</p>
        </div>
        <Button
          type="button"
          disabled={busy || recoveryStatus === 'checking'}
          onClick={() => guard(() => open(null))}
        >
          <Plus aria-hidden="true" />
          {t('new')}
        </Button>
      </div>
      {pending && (
        <div
          className="announcement-confirm"
          role="group"
          aria-labelledby="announcement-discard-title"
          onKeyDown={(event) => {
            if (event.key === 'Escape' && !busy) setPending(null);
          }}
        >
          <strong id="announcement-discard-title">
            {t(pending.reload ? 'reloadConfirm' : 'discardTitle')}
          </strong>
          <p>{t(pending.reload ? 'reloadHint' : 'discardHint')}</p>
          <div className="actions">
            <Button
              type="button"
              variant="outline"
              autoFocus
              disabled={busy}
              onClick={() => {
                const action = pending.action;
                if (!pending.reload) close();
                setPending(null);
                action();
              }}
            >
              {t(pending.reload ? 'reloadLatest' : 'discard')}
            </Button>
            <Button type="button" variant="ghost" disabled={busy} onClick={() => setPending(null)}>
              {t('keepEditing')}
            </Button>
          </div>
        </div>
      )}
      <div className={`announcement-admin-columns${editing ? ' is-editing' : ''}`}>
        <section
          className="glass announcement-management-list"
          aria-label={t('manageTitle')}
          aria-busy={isLoading}
        >
          <div className="announcement-list-controls">
            <div className="field">
              <label htmlFor="announcement-status-filter">{t('statusFilter')}</label>
              <FieldSelect
                id="announcement-status-filter"
                className="native-select"
                value={status}
                disabled={busy}
                onValueChange={(value) => select({ status: value })}
                options={[
                  { value: 'all', label: c('all') },
                  ...statuses.map((value) => ({ value, label: t(value) })),
                ]}
              />
            </div>
            {data && !loadError && (
              <p className="hint" role="status">
                {t('results', { count: data.total })}
              </p>
            )}
          </div>
          {isLoading ? (
            <Loading />
          ) : loadError ? (
            <AnnouncementError error={loadError} retry={() => mutate()} />
          ) : !data?.items.length ? (
            <Empty title={t('adminEmpty')} description={t('adminEmptyDescription')} />
          ) : (
            <ul className="announcement-admin-rows">
              {data.items.map((announcement) => (
                <li key={announcement.id}>
                  <button
                    type="button"
                    className="announcement-select"
                    aria-pressed={editing && selected?.id === announcement.id}
                    disabled={busy}
                    onClick={() => {
                      if (selected?.id !== announcement.id || !editing)
                        guard(() => open(announcement));
                    }}
                  >
                    <span className="announcement-select-heading">
                      <strong>{announcement.title}</strong>
                      <span
                        className={`announcement-status announcement-status-${announcement.status}`}
                      >
                        {t(announcement.status)}
                      </span>
                    </span>
                    <span className="announcement-select-meta">
                      {announcement.author.username} ·{' '}
                      <AnnouncementDate value={announcement.updated_at} />
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {data && !loadError && (
            <Pagination
              page={page}
              total={data.total}
              pageSize={data.page_size}
              onChange={(value) => select({ page: value })}
            />
          )}
          {refreshFailed && <AnnouncementError error={new Error()} retry={() => refreshList()} />}
        </section>
        {editing && recoveryStatus !== 'ready' && (
          <div className="glass announcement-editor" aria-busy={recoveryStatus === 'checking'}>
            {recoveryStatus === 'checking' ? (
              <Loading />
            ) : (
              <AnnouncementError
                error={recoveryError}
                retry={() => {
                  setRecoveryStatus('checking');
                  setRecoveryError(null);
                  setRecoveryAttempt((attempt) => attempt + 1);
                }}
              />
            )}
          </div>
        )}
        {editing && recoveryStatus === 'ready' && (
          <form
            className="glass announcement-editor"
            onSubmit={(event) => {
              event.preventDefault();
              void save(selected?.status === 'published' ? 'published' : 'draft');
            }}
          >
            <div className="announcement-editor-heading">
              <h3>{t(selected ? 'edit' : 'new')}</h3>
              <span
                className={`announcement-status announcement-status-${selected?.status ?? 'draft'}`}
              >
                {t(selected?.status ?? 'draft')}
              </span>
            </div>
            <p className="hint announcement-publication-hint">
              {t(selected?.status === 'published' ? 'publishedHint' : 'draftHint')}
            </p>
            {recovered && (
              <p className="announcement-recovered" role="status">
                {t('recoveredDraft')}
              </p>
            )}
            <div className="field">
              <label htmlFor="announcement-title">{t('titleField')}</label>
              <Input
                ref={titleRef}
                id="announcement-title"
                name="title"
                value={title}
                disabled={busy}
                maxLength={320}
                onChange={(event) => {
                  setTitle(event.target.value);
                  setSuccess('');
                }}
                aria-describedby="announcement-title-hint"
              />
              <p className="hint" id="announcement-title-hint">
                {t('titleHint', { count: Array.from(title).length })}
              </p>
            </div>
            <div className="field">
              <label htmlFor="announcement-body">{t('bodyField')}</label>
              <Textarea
                id="announcement-body"
                name="body"
                value={body}
                disabled={busy}
                maxLength={40000}
                rows={13}
                onChange={(event) => {
                  setBody(event.target.value);
                  setSuccess('');
                }}
                aria-describedby="announcement-body-hint"
              />
              <p className="hint" id="announcement-body-hint">
                {t('bodyHint', { count: Array.from(body).length })}
              </p>
            </div>
            {dirty && (
              <p className="announcement-unsaved" role="status">
                {t('unsaved')}
              </p>
            )}
            {error && (
              <p className="form-error" role="alert">
                {error}
              </p>
            )}
            {success && (
              <p className="form-success" role="status">
                {success}
              </p>
            )}
            {conflict && (
              <div className="announcement-conflict">
                <p>{t('conflictHint')}</p>
                <Button
                  type="button"
                  variant="outline"
                  disabled={busy}
                  onClick={() =>
                    setPending({
                      action: () => {
                        void reloadLatest();
                      },
                      reload: true,
                    })
                  }
                >
                  <RefreshCw aria-hidden="true" />
                  {t('reloadLatest')}
                </Button>
              </div>
            )}
            <div className="actions announcement-editor-actions">
              <Button type="button" disabled={busy || conflict} onClick={() => save('published')}>
                {busy && <LoaderCircle className="spin" aria-hidden="true" />}
                {t(selected?.status === 'published' ? 'savePublished' : 'publish')}
              </Button>
              {selected?.status !== 'published' && (
                <Button type="submit" variant="outline" disabled={busy || conflict}>
                  {t('saveDraft')}
                </Button>
              )}
              {selected?.status === 'published' && (
                <Button
                  type="button"
                  variant="outline"
                  disabled={busy || conflict}
                  onClick={() => setWithdrawConfirm(true)}
                >
                  {t('withdraw')}
                </Button>
              )}
              <Button type="button" variant="ghost" disabled={busy} onClick={() => guard(close)}>
                {c('cancel')}
              </Button>
            </div>
            {withdrawConfirm && (
              <div
                className="announcement-confirm"
                role="group"
                aria-labelledby="announcement-withdraw-title"
              >
                <strong id="announcement-withdraw-title">{t('withdrawConfirm')}</strong>
                <p>{t('withdrawHint')}</p>
                <div className="actions">
                  <Button
                    type="button"
                    variant="outline"
                    disabled={busy || conflict}
                    onClick={() => save('archived')}
                  >
                    {t('withdraw')}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => setWithdrawConfirm(false)}
                  >
                    {c('cancel')}
                  </Button>
                </div>
              </div>
            )}
          </form>
        )}
      </div>
    </div>
  );
}

function AnnouncementDate({ value }: { value: string }) {
  const f = useFormatter();
  return <time dateTime={value}>{f.dateTime(new Date(value), { dateStyle: 'medium' })}</time>;
}
