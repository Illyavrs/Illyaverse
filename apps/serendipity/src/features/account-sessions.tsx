'use client';

import { useEffect, useRef, useState } from 'react';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR from 'swr';
import { Check, Gamepad2, Globe, LoaderCircle, LogOut, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { useSession } from '@/components/providers';
import { useNavigationProgress } from '@/components/navigation-motion';
import { api, ApiError } from '@/lib/api';
import styles from './account-sessions.module.css';

type LoginSession = {
  id: string;
  client: 'web' | 'stable' | 'lazer';
  current: boolean;
  created_at: string;
  expires_at: string;
  last_seen_at: string | null;
};
type SessionList = { sessions: LoginSession[]; has_more: boolean };
type RevokeResult = { ok: boolean; current_revoked: boolean };
type Confirmation = { kind: 'others' } | { kind: 'current'; session: LoginSession };

const sessionKey = (session: LoginSession) => `${session.client}/${session.id}`;
const endpoint = '/me/sessions';
const loadSessions = () => api<SessionList>(endpoint, { signal: AbortSignal.timeout(15000) });

export function AccountSessionControls() {
  const t = useTranslations('AccountSessions');
  const { user, clear } = useSession();
  const { navigate } = useNavigationProgress();
  const {
    data,
    error: loadError,
    isLoading,
    mutate,
  } = useSWR<SessionList>(user ? endpoint : null, loadSessions, {
    shouldRetryOnError: false,
    revalidateOnFocus: true,
  });
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [refreshing, setRefreshing] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const [feedback, setFeedback] = useState<'revoked' | 'othersRevoked' | null>(null);
  const mounted = useRef(true);
  const requests = useRef(new Set<string>());
  const removed = useRef(new Set<string>());
  const refreshSequence = useRef(0);
  const signingOut = useRef(false);
  const confirmationTrigger = useRef<HTMLButtonElement | null>(null);
  const refreshButton = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  async function signedOut() {
    if (signingOut.current) return;
    signingOut.current = true;
    await clear();
    navigate('/login', { replace: true });
  }

  useEffect(() => {
    if (loadError instanceof ApiError && loadError.status === 401 && !signingOut.current) {
      signingOut.current = true;
      void clear().then(() => navigate('/login', { replace: true }));
    }
  }, [loadError, clear, navigate]);

  function failureMessage(failure: unknown) {
    if (failure instanceof ApiError) {
      if (failure.status === 429) return 'rateLimited';
      if (failure.status === 404) return 'alreadyRevoked';
    }
    return 'revokeError';
  }

  async function refreshList() {
    const sequence = ++refreshSequence.current;
    if (mounted.current) setRefreshing(true);
    try {
      // A failed SWR revalidation can resolve with old data. Fetch explicitly so
      // the UI can distinguish a saved revocation from a failed list refresh.
      const latest = await api<SessionList>(endpoint, { signal: AbortSignal.timeout(15000) });
      if (!mounted.current || sequence !== refreshSequence.current) return;
      await mutate(
        {
          ...latest,
          sessions: latest.sessions.filter((session) => !removed.current.has(sessionKey(session))),
        },
        { revalidate: false },
      );
      setRefreshFailed(false);
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 401) {
        await signedOut();
      } else if (mounted.current && sequence === refreshSequence.current) {
        setRefreshFailed(true);
      }
    } finally {
      if (mounted.current && sequence === refreshSequence.current) setRefreshing(false);
    }
  }

  async function revoke(session: LoginSession) {
    const key = sessionKey(session);
    if (requests.current.has(key) || requests.current.has('others')) return;
    requests.current.add(key);
    setPending(new Set(requests.current));
    setActionError(null);
    setFeedback(null);
    try {
      const result = await api<RevokeResult>(`${endpoint}/${key}`, {
        method: 'DELETE',
        signal: AbortSignal.timeout(15000),
      });
      if (result.current_revoked) {
        await signedOut();
        return;
      }
      removed.current.add(key);
      if (!mounted.current) return;
      await mutate(
        (previous) =>
          previous && {
            ...previous,
            sessions: previous.sessions.filter((item) => !removed.current.has(sessionKey(item))),
          },
        { revalidate: false },
      );
      setFeedback('revoked');
      await refreshList();
      if (mounted.current) setConfirmation(null);
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 401) await signedOut();
      else if (mounted.current) {
        const reason = failureMessage(failure);
        setActionError(reason);
        if (reason === 'alreadyRevoked') await refreshList();
      }
    } finally {
      requests.current.delete(key);
      if (mounted.current) setPending(new Set(requests.current));
    }
  }

  async function revokeOthers() {
    if (requests.current.size) return;
    requests.current.add('others');
    setPending(new Set(requests.current));
    setActionError(null);
    setFeedback(null);
    let completed = false;
    try {
      await api(`${endpoint}/revoke-others`, {
        method: 'POST',
        signal: AbortSignal.timeout(15000),
      });
      completed = true;
      if (!mounted.current) return;
      for (const session of data?.sessions ?? []) {
        if (!session.current) removed.current.add(sessionKey(session));
      }
      await mutate(
        (previous) =>
          previous && {
            ...previous,
            sessions: previous.sessions.filter((session) => session.current),
            has_more: false,
          },
        { revalidate: false },
      );
      setFeedback('othersRevoked');
      await refreshList();
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 401) await signedOut();
      else if (mounted.current) setActionError(failureMessage(failure));
    } finally {
      requests.current.delete('others');
      if (mounted.current) {
        setPending(new Set(requests.current));
        // Successful revocation closes only after the refresh has settled, so
        // the originating control is enabled when Radix restores focus.
        if (completed) setConfirmation(null);
      }
    }
  }

  if (!user) return null;
  const hasOthers = Boolean(data?.has_more || data?.sessions.some((session) => !session.current));
  const dialogPending =
    confirmation?.kind === 'others'
      ? pending.has('others')
      : Boolean(confirmation && pending.has(sessionKey(confirmation.session)));

  return (
    <section className={styles.section} aria-labelledby="account-sessions-heading">
      <div className={styles.heading}>
        <h3 id="account-sessions-heading">{t('title')}</h3>
        <div className={styles.actions}>
          <Button
            ref={refreshButton}
            type="button"
            variant="ghost"
            size="sm"
            disabled={refreshing || isLoading || pending.size > 0}
            onClick={() => void refreshList()}
          >
            <RefreshCw aria-hidden="true" className={refreshing ? 'spin' : undefined} />
            {t('refresh')}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!hasOthers || pending.size > 0 || Boolean(loadError)}
            onClick={(event) => {
              confirmationTrigger.current = event.currentTarget;
              setActionError(null);
              setConfirmation({ kind: 'others' });
            }}
          >
            <LogOut aria-hidden="true" />
            {t('revokeOthers')}
          </Button>
        </div>
      </div>
      <p className={styles.hint}>{t('description')}</p>
      {isLoading ? (
        <p className={styles.state} role="status">
          <LoaderCircle className="spin" aria-hidden="true" />
          {t('loading')}
        </p>
      ) : loadError && !data ? (
        <p className={`${styles.feedback} ${styles.error}`} role="alert">
          {t('loadError')}
        </p>
      ) : data?.sessions.length ? (
        <ul className={styles.list} aria-label={t('listLabel')}>
          {data.sessions.map((session) => {
            const working = pending.has(sessionKey(session));
            const Icon = session.client === 'web' ? Globe : Gamepad2;
            return (
              <li
                className={styles.row}
                key={sessionKey(session)}
                data-session-client={session.client}
              >
                <div className={styles.identity}>
                  <Icon aria-hidden="true" />
                  <strong>{t(session.client)}</strong>
                  {session.current && <span className={styles.current}>{t('current')}</span>}
                </div>
                <dl className={styles.dates}>
                  <div>
                    <dt>{t('created')}</dt>
                    <dd>
                      <SessionDate value={session.created_at} />
                    </dd>
                  </div>
                  {session.last_seen_at && (
                    <div>
                      <dt>{t('lastSeen')}</dt>
                      <dd>
                        <SessionDate value={session.last_seen_at} />
                      </dd>
                    </div>
                  )}
                  <div>
                    <dt>{t('expires')}</dt>
                    <dd>
                      <SessionDate value={session.expires_at} />
                    </dd>
                  </div>
                </dl>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className={styles.rowAction}
                  disabled={working || pending.has('others')}
                  aria-label={t(session.current ? 'revokeCurrentLabel' : 'revokeLabel', {
                    client: t(session.client),
                  })}
                  onClick={(event) => {
                    if (session.current) {
                      confirmationTrigger.current = event.currentTarget;
                      setActionError(null);
                      setConfirmation({ kind: 'current', session });
                    } else void revoke(session);
                  }}
                >
                  {working && <LoaderCircle className="spin" aria-hidden="true" />}
                  {t(working ? 'working' : 'revoke')}
                </Button>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className={styles.state}>{t('empty')}</p>
      )}
      {data?.has_more && <p className={styles.hint}>{t('limited')}</p>}
      {(refreshFailed || (loadError && data)) && (
        <p className={`${styles.feedback} ${styles.error}`} role="alert">
          {t('refreshError')}
        </p>
      )}
      {actionError && !confirmation && (
        <p className={`${styles.feedback} ${styles.error}`} role="alert">
          {t(actionError)}
        </p>
      )}
      {feedback && (
        <p className={`${styles.feedback} ${styles.success}`} role="status">
          <Check aria-hidden="true" />
          {t(feedback)}
        </p>
      )}
      <AlertDialog
        open={confirmation !== null}
        onOpenChange={(open) => {
          if (!open && !dialogPending) setConfirmation(null);
        }}
      >
        <AlertDialogContent
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            const target = confirmationTrigger.current;
            if (target?.isConnected && !target.disabled) target.focus();
            else refreshButton.current?.focus();
          }}
        >
          <AlertDialogTitle>
            {t(confirmation?.kind === 'others' ? 'othersTitle' : 'currentTitle')}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t(confirmation?.kind === 'others' ? 'othersHint' : 'currentHint')}
          </AlertDialogDescription>
          {actionError && (
            <p className="form-error" role="alert">
              {t(actionError)}
            </p>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={dialogPending}>{t('cancel')}</AlertDialogCancel>
            <AlertDialogAction
              disabled={dialogPending || (confirmation?.kind === 'others' && pending.size > 0)}
              onClick={(event) => {
                event.preventDefault();
                if (confirmation?.kind === 'others') void revokeOthers();
                else if (confirmation) void revoke(confirmation.session);
              }}
            >
              {dialogPending && <LoaderCircle className="spin" aria-hidden="true" />}
              {t(dialogPending ? 'working' : 'confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}

function SessionDate({ value }: { value: string }) {
  const formatter = useFormatter();
  const t = useTranslations('AccountSessions');
  const date = new Date(value);
  if (!Number.isFinite(date.valueOf())) return <span>{t('dateUnavailable')}</span>;
  return (
    <time dateTime={value}>
      {formatter.dateTime(date, { dateStyle: 'medium', timeStyle: 'short' })}
    </time>
  );
}
