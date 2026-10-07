'use client';

import { useEffect, useEffectEvent, useRef, useState, type FormEvent, type ReactNode } from 'react';
import Link from 'next/link';
import { useLocale, useTranslations } from 'next-intl';
import useSWR from 'swr';
import { ArrowRight, Check, Disc3, LoaderCircle, Mail } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useSession } from '@/components/providers';
import { api, ApiError, fetcher } from '@/lib/api';

type Capabilities = { password_reset: boolean; email_verification: boolean };
type RecoveryFailure =
  'unavailable' | 'rateLimited' | 'requestError' | 'linkInvalid' | 'passwordInvalid';

function recoveryFailure(error: unknown): RecoveryFailure {
  if (error instanceof ApiError) {
    if (error.status === 503) return 'unavailable';
    if (error.status === 429) return 'rateLimited';
    if (error.code === 'invalid_recovery_token') return 'linkInvalid';
  }
  return 'requestError';
}

function RecoveryLayout({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  const t = useTranslations('recovery');
  return (
    <div className="auth-layout">
      <div className="auth-art" aria-hidden="true">
        <div className="auth-art-caption">
          <Disc3 />
          <span>Illyaverse</span>
        </div>
      </div>
      <section className="auth-form glass">
        <h1>{title}</h1>
        <p className="lede">{description}</p>
        {children}
        <p className="auth-switch">
          <Link href="/login">{t('backToLogin')}</Link>
        </p>
      </section>
    </div>
  );
}

function CapabilityMessage({
  loading,
  failed,
  available,
  retry,
}: {
  loading: boolean;
  failed: boolean;
  available: boolean;
  retry: () => void;
}) {
  const t = useTranslations('recovery');
  const c = useTranslations('common');
  if (loading)
    return (
      <p className="hint" role="status">
        {c('loading')}
      </p>
    );
  if (failed)
    return (
      <div>
        <p className="form-error" role="alert">
          {t('requestError')}
        </p>
        <Button type="button" variant="outline" onClick={retry}>
          {c('retry')}
        </Button>
      </div>
    );
  if (!available)
    return (
      <p className="hint" role="status">
        {t('unavailable')}
      </p>
    );
  return null;
}

// The fragment never reaches the server. Clear it before any intentional API action,
// and keep the credential only in this mounted component's memory.
function useRecoveryToken(onNewLink: () => void) {
  const captured = useRef<string | null>(null);
  const revision = useRef(0);
  const active = useRef(false);
  const notifyNewLink = useEffectEvent(onNewLink);
  const [snapshot, setSnapshot] = useState<{ token: string | null; revision: number }>({
    token: null,
    revision: 0,
  });
  useEffect(() => {
    active.current = true;
    function publish() {
      const value = { token: captured.current, revision: revision.current };
      queueMicrotask(() => {
        if (active.current && revision.current === value.revision) setSnapshot(value);
      });
    }
    function capture() {
      const fragment = new URLSearchParams(window.location.hash.slice(1));
      const values = fragment.getAll('token');
      captured.current = values.length === 1 && /^[a-f0-9]{64}$/.test(values[0]) ? values[0] : '';
      revision.current += 1;
      window.history.replaceState(
        window.history.state,
        '',
        window.location.pathname + window.location.search,
      );
      notifyNewLink();
      publish();
    }
    if (captured.current === null) capture();
    else publish();
    window.addEventListener('hashchange', capture);
    return () => {
      active.current = false;
      window.removeEventListener('hashchange', capture);
    };
  }, []);
  function isCurrent(expectedRevision: number) {
    return active.current && revision.current === expectedRevision;
  }
  function clearToken(expectedRevision: number) {
    if (!isCurrent(expectedRevision)) return;
    captured.current = '';
    setSnapshot({ token: '', revision: expectedRevision });
  }
  return { ...snapshot, clearToken, isCurrent };
}

export function ForgotPasswordPage() {
  const t = useTranslations('recovery');
  const a = useTranslations('auth');
  const locale = useLocale();
  const {
    data,
    error: capabilityError,
    isLoading,
    mutate,
  } = useSWR<Capabilities>('/auth/capabilities', fetcher);
  const [busy, setBusy] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState<RecoveryFailure | null>(null);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!data?.password_reset || busy) return;
    const email = String(new FormData(event.currentTarget).get('email') ?? '').trim();
    setBusy(true);
    setError(null);
    try {
      await api('/auth/recovery/request', {
        method: 'POST',
        body: JSON.stringify({ email, locale }),
      });
      setAccepted(true);
    } catch (failure) {
      setError(recoveryFailure(failure));
    } finally {
      setBusy(false);
    }
  }
  return (
    <RecoveryLayout title={t('forgotTitle')} description={t('forgotDescription')}>
      {accepted ? (
        <div role="status">
          <p className="form-success">
            <Check aria-hidden="true" />
            {t('requestAccepted')}
          </p>
          <p className="hint">{t('requestAcceptedHint')}</p>
        </div>
      ) : (
        <form onSubmit={submit} aria-busy={busy}>
          <div className="field">
            <label htmlFor="recovery-email">{a('email')}</label>
            <Input
              id="recovery-email"
              name="email"
              type="email"
              autoComplete="email"
              maxLength={254}
              required
              disabled={busy}
            />
          </div>
          <CapabilityMessage
            loading={isLoading}
            failed={!!capabilityError}
            available={!!data?.password_reset}
            retry={() => void mutate()}
          />
          {error && (
            <p className="form-error" role="alert">
              {t(error)}
            </p>
          )}
          <Button type="submit" disabled={busy || !data?.password_reset} className="w-full">
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" />
            ) : (
              <Mail aria-hidden="true" />
            )}
            {t(busy ? 'working' : 'requestReset')}
          </Button>
        </form>
      )}
    </RecoveryLayout>
  );
}

export function ResetPasswordPage() {
  const t = useTranslations('recovery');
  const a = useTranslations('auth');
  const c = useTranslations('common');
  const { refresh } = useSession();
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<RecoveryFailure | 'passwordMismatch' | null>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const { token, revision, clearToken, isCurrent } = useRecoveryToken(() => {
    formRef.current?.reset();
    setBusy(false);
    setDone(false);
    setError(null);
  });
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token || busy || !isCurrent(revision)) return;
    const form = event.currentTarget;
    const data = new FormData(form);
    const password = String(data.get('password') ?? '');
    if (password !== data.get('confirm_password')) {
      setError('passwordMismatch');
      return;
    }
    setBusy(true);
    setError(null);
    const attempt = revision;
    try {
      await api('/auth/recovery/reset', {
        method: 'POST',
        body: JSON.stringify({ token, password }),
      });
      if (!isCurrent(attempt)) return;
      form.reset();
      clearToken(attempt);
      setDone(true);
      await refresh().catch(() => undefined);
    } catch (failure) {
      if (!isCurrent(attempt)) return;
      const reason =
        failure instanceof ApiError && failure.code === 'invalid_request'
          ? 'passwordInvalid'
          : recoveryFailure(failure);
      setError(reason);
      if (reason === 'linkInvalid') clearToken(attempt);
    } finally {
      if (isCurrent(attempt)) setBusy(false);
    }
  }
  return (
    <RecoveryLayout title={t('resetTitle')} description={t('resetDescription')}>
      {done ? (
        <div role="status">
          <p className="form-success">
            <Check aria-hidden="true" />
            {t('resetSuccess')}
          </p>
          <p className="hint">{t('resetSuccessHint')}</p>
        </div>
      ) : token === null ? (
        <p className="hint" role="status">
          {c('loading')}
        </p>
      ) : !token ? (
        <div>
          <p className="form-error" role="alert">
            {t('linkInvalid')}
          </p>
          <p className="auth-switch">
            <Link href="/forgot-password">{t('requestAnother')}</Link>
          </p>
        </div>
      ) : (
        <form ref={formRef} onSubmit={submit} aria-busy={busy}>
          <div className="field">
            <label htmlFor="reset-password">{t('newPassword')}</label>
            <Input
              id="reset-password"
              name="password"
              type="password"
              autoComplete="new-password"
              minLength={12}
              maxLength={128}
              required
              disabled={busy}
              aria-describedby="reset-password-hint"
            />
            <p id="reset-password-hint" className="hint">
              {a('passwordHint')}
            </p>
          </div>
          <div className="field">
            <label htmlFor="confirm-password">{t('confirmPassword')}</label>
            <Input
              id="confirm-password"
              name="confirm_password"
              type="password"
              autoComplete="new-password"
              minLength={12}
              maxLength={128}
              required
              disabled={busy}
            />
          </div>
          {error && (
            <p className="form-error" role="alert">
              {t(error)}
            </p>
          )}
          <Button type="submit" disabled={busy} className="w-full">
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" />
            ) : (
              <ArrowRight aria-hidden="true" />
            )}
            {t(busy ? 'working' : 'resetPassword')}
          </Button>
        </form>
      )}
    </RecoveryLayout>
  );
}

export function VerifyEmailPage() {
  const t = useTranslations('recovery');
  const c = useTranslations('common');
  const { refresh } = useSession();
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<RecoveryFailure | null>(null);
  const { token, revision, clearToken, isCurrent } = useRecoveryToken(() => {
    setBusy(false);
    setDone(false);
    setError(null);
  });
  async function confirm() {
    if (!token || busy || !isCurrent(revision)) return;
    setBusy(true);
    setError(null);
    const attempt = revision;
    try {
      await api('/auth/email/verify', { method: 'POST', body: JSON.stringify({ token }) });
      if (!isCurrent(attempt)) return;
      clearToken(attempt);
      setDone(true);
      await refresh().catch(() => undefined);
    } catch (failure) {
      if (!isCurrent(attempt)) return;
      const reason = recoveryFailure(failure);
      setError(reason);
      if (reason === 'linkInvalid') clearToken(attempt);
    } finally {
      if (isCurrent(attempt)) setBusy(false);
    }
  }
  return (
    <RecoveryLayout title={t('verifyTitle')} description={t('verifyDescription')}>
      {done ? (
        <div role="status">
          <p className="form-success">
            <Check aria-hidden="true" />
            {t('verifySuccess')}
          </p>
          <p className="auth-switch">
            <Link href="/settings?tab=account">{t('backToSettings')}</Link>
          </p>
        </div>
      ) : token === null ? (
        <p className="hint" role="status">
          {c('loading')}
        </p>
      ) : !token ? (
        <div>
          <p className="form-error" role="alert">
            {t('linkInvalid')}
          </p>
          <p className="hint">{t('verificationInvalidHint')}</p>
          <p className="auth-switch">
            <Link href="/settings?tab=account">{t('backToSettings')}</Link>
          </p>
        </div>
      ) : (
        <div aria-busy={busy}>
          <p className="hint mb-5">{t('verifyHint')}</p>
          {error && (
            <p className="form-error" role="alert">
              {t(error)}
            </p>
          )}
          <Button type="button" onClick={confirm} disabled={busy} className="w-full">
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" />
            ) : (
              <Check aria-hidden="true" />
            )}
            {t(busy ? 'working' : 'verifyEmail')}
          </Button>
        </div>
      )}
    </RecoveryLayout>
  );
}

export function EmailVerificationControls() {
  const t = useTranslations('recovery');
  const { user } = useSession();
  const {
    data,
    error: capabilityError,
    isLoading,
    mutate,
  } = useSWR<Capabilities>(user && !user.email_verified ? '/auth/capabilities' : null, fetcher);
  const [busy, setBusy] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState<RecoveryFailure | null>(null);
  async function request() {
    if (!data?.email_verification || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api('/auth/email/request', { method: 'POST' });
      setAccepted(true);
    } catch (failure) {
      setError(recoveryFailure(failure));
    } finally {
      setBusy(false);
    }
  }
  if (!user) return null;
  return (
    <div className="mb-8 space-y-3" aria-busy={busy}>
      <h3 className="font-extrabold">{t('verificationTitle')}</h3>
      {user.email_verified ? (
        <p className="form-success">
          <Check aria-hidden="true" />
          {t('verified')}
        </p>
      ) : (
        <>
          <p className="hint">{t('notVerified')}</p>
          <CapabilityMessage
            loading={isLoading}
            failed={!!capabilityError}
            available={!!data?.email_verification}
            retry={() => void mutate()}
          />
          {accepted && (
            <p className="form-success" role="status">
              {t('verificationRequested')}
            </p>
          )}
          {error && (
            <p className="form-error" role="alert">
              {t(error)}
            </p>
          )}
          <Button
            type="button"
            variant="outline"
            onClick={request}
            disabled={busy || accepted || !data?.email_verification}
          >
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" />
            ) : (
              <Mail aria-hidden="true" />
            )}
            {t(busy ? 'working' : 'requestVerification')}
          </Button>
          {accepted && <p className="hint">{t('verificationRequestedHint')}</p>}
        </>
      )}
    </div>
  );
}
