'use client';
import { useState, type FormEvent } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useLocale, useTranslations } from 'next-intl';
import { ArrowRight, Disc3, LoaderCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useSession } from '@/components/providers';
import { api } from '@/lib/api';
import type { User } from '@/lib/types';
export function AuthPage({ register = false }: { register?: boolean }) {
  const t = useTranslations('auth');
  const locale = useLocale();
  const { refresh } = useSession();
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setBusy(true);
    setError('');
    const data = new FormData(e.currentTarget);
    try {
      const result = await api<{ user: User }>(register ? '/auth/register' : '/auth/login', {
        method: 'POST',
        body: JSON.stringify({ ...Object.fromEntries(data), ...(register ? { locale } : {}) }),
      });
      await refresh();
      router.push(register ? '/connect' : `/players/${result.user.id}`);
      router.refresh();
    } catch {
      setError(t('error'));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="auth-layout">
      <div className="auth-art" aria-hidden="true">
        <div className="auth-art-caption">
          <Disc3 />
          <span>Illyaverse</span>
        </div>
      </div>
      <section className="auth-form glass">
        <h1>{t(register ? 'registerTitle' : 'loginTitle')}</h1>
        <p className="lede">{t(register ? 'registerDescription' : 'loginDescription')}</p>
        <form onSubmit={submit}>
          <div className="field">
            <label htmlFor="username">{t('username')}</label>
            <Input
              id="username"
              name="username"
              autoComplete="username"
              required
              minLength={3}
              maxLength={24}
            />
          </div>
          {register && (
            <div className="field">
              <label htmlFor="email">{t('email')}</label>
              <Input
                id="email"
                name="email"
                type="email"
                autoComplete="email"
                required
                maxLength={254}
              />
            </div>
          )}
          <div className="field">
            <label htmlFor="password">{t('password')}</label>
            <Input
              id="password"
              name="password"
              type="password"
              autoComplete={register ? 'new-password' : 'current-password'}
              minLength={register ? 12 : undefined}
              maxLength={128}
              required
            />
            {register && <p className="hint">{t('passwordHint')}</p>}
          </div>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <Button type="submit" disabled={busy} className="w-full">
            {busy ? <LoaderCircle className="spin" /> : <ArrowRight />}
            {t(busy ? 'working' : register ? 'register' : 'login')}
          </Button>
        </form>
        <p className="auth-switch">
          {t(register ? 'hasAccount' : 'noAccount')}{' '}
          <Link href={register ? '/login' : '/register'}>{t(register ? 'login' : 'register')}</Link>
        </p>
        <p className="auth-notice">{t('notice')}</p>
      </section>
    </div>
  );
}
