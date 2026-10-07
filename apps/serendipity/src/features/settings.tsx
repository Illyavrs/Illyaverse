'use client';
import { useState, type FormEvent } from 'react';
import Image from 'next/image';
import { useLocale, useTranslations } from 'next-intl';
import { ImagePlus, RotateCcw, Check, LoaderCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Slider } from '@/components/ui/slider';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useAppearance, useSession } from '@/components/providers';
import { PageHeading, Protected, Avatar, Loading, LoadError } from '@/components/common';
import { api } from '@/lib/api';
import { isLocale } from '@/i18n/config';
import { RelationshipList } from './social';
import { EmailVerificationControls } from './recovery';

export function AppearanceControls({ compact = false }: { compact?: boolean }) {
  const t = useTranslations('settings');
  const c = useTranslations('common');
  const { preferences: p, ready, loadError, reload, update, save, reset } = useAppearance();
  const { user } = useSession();
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [error, setError] = useState('');
  async function persist() {
    setBusy(true);
    setError('');
    try {
      await save();
      setFeedback(c('saved'));
    } catch {
      setError(c('serviceError'));
    } finally {
      setBusy(false);
    }
  }
  async function upload(file?: File) {
    if (!file) return;
    setBusy(true);
    setError('');
    const data = new FormData();
    data.set('file', file);
    data.set('kind', 'background');
    try {
      const result = await api<{ url: string }>('/me/media?kind=background', {
        method: 'POST',
        body: data,
      });
      update({ background_url: result.url });
      setFeedback(t('imageUploaded'));
    } catch {
      setError(c('serviceError'));
    } finally {
      setBusy(false);
    }
  }
  if (loadError) return <LoadError retry={reload} />;
  if (!ready) return <Loading />;
  return (
    <div className={`appearance-controls ${compact ? 'compact' : ''}`}>
      <div className="field">
        <label>{t('theme')}</label>
        <div className="segmented">
          {(['dark', 'light', 'system'] as const).map((mode) => (
            <button
              type="button"
              key={mode}
              aria-pressed={p.mode === mode}
              onClick={() => update({ mode })}
            >
              {t(mode)}
            </button>
          ))}
        </div>
      </div>
      <div className="field">
        <label htmlFor={compact ? 'quick-accent' : 'accent'}>{t('accent')}</label>
        <div className="colour-field">
          <input
            type="color"
            id={compact ? 'quick-accent' : 'accent'}
            value={p.accent_color}
            onChange={(e) => update({ accent_color: e.target.value })}
          />
          <span>{p.accent_color.toUpperCase()}</span>
          <div className="swatches">
            {['#f28eb6', '#a8a2f2', '#7dd8c5', '#f6c889'].map((color) => (
              <button
                key={color}
                type="button"
                style={{ background: color }}
                aria-label={color}
                aria-pressed={p.accent_color === color}
                onClick={() => update({ accent_color: color })}
              />
            ))}
          </div>
        </div>
      </div>
      <div className="field">
        <label>{t('background')}</label>
        <div
          className="background-preview"
          style={{
            backgroundImage: p.background_url
              ? `url(${JSON.stringify(p.background_url)})`
              : undefined,
          }}
        />
        <p className="hint">{t('backgroundHint')}</p>
        <div className="actions">
          <label className={`upload-button ${!user ? 'disabled' : ''}`}>
            <ImagePlus />
            {t('uploadImage')}
            <input
              type="file"
              accept="image/png,image/jpeg,image/webp"
              disabled={!user || busy}
              onChange={(e) => upload(e.target.files?.[0])}
            />
          </label>
          {p.background_url && (
            <Button variant="ghost" onClick={() => update({ background_url: null })}>
              {t('removeBackground')}
            </Button>
          )}
        </div>
      </div>
      {(
        [
          { key: 'background_blur', label: 'blur', max: 48, unit: 'px' },
          { key: 'background_dim', label: 'dim', max: 100, unit: '%' },
          { key: 'border_radius', label: 'radius', max: 40, unit: 'px' },
        ] as const
      ).map(({ key, label, max, unit }) => (
        <div className="field slider-field" key={key}>
          <div>
            <label htmlFor={`${compact ? 'quick-' : ''}${key}`}>{t(label)}</label>
            <output>
              {key === 'background_dim' ? Math.round(p[key] * 100) : p[key]}
              {unit}
            </output>
          </div>
          <Slider
            id={`${compact ? 'quick-' : ''}${key}`}
            min={0}
            max={max}
            step={1}
            value={[key === 'background_dim' ? Math.round(p[key] * 100) : p[key]]}
            onValueChange={([value]) =>
              update({ [key]: key === 'background_dim' ? value / 100 : value })
            }
            aria-label={t(label)}
          />
        </div>
      ))}
      <p className="hint">{user ? t('preview') : t('guest')}</p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {feedback && (
        <p className="form-success" role="status">
          <Check />
          {feedback}
        </p>
      )}
      <div className="actions">
        <Button onClick={persist} disabled={busy}>
          {busy ? <LoaderCircle className="spin" /> : <Check />}
          {c('save')}
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            reset();
            setFeedback(t('resetDone'));
          }}
        >
          <RotateCcw />
          {t('reset')}
        </Button>
      </div>
    </div>
  );
}

function ProfileControls() {
  const t = useTranslations('settings');
  const c = useTranslations('common');
  const { user, refresh } = useSession();
  const [bio, setBio] = useState(user?.bio ?? '');
  const [country, setCountry] = useState(user?.country ?? 'TW');
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [error, setError] = useState('');
  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError('');
    try {
      await api('/me/profile', { method: 'PATCH', body: JSON.stringify({ bio, country }) });
      await refresh();
      setFeedback(c('saved'));
    } catch {
      setError(c('serviceError'));
    } finally {
      setBusy(false);
    }
  }
  async function media(file: File | undefined, kind: string) {
    if (!file) return;
    setBusy(true);
    setError('');
    const data = new FormData();
    data.set('file', file);
    data.set('kind', kind);
    try {
      await api(`/me/media?kind=${kind}`, { method: 'POST', body: data });
      await refresh();
      setFeedback(t('imageUploaded'));
    } catch {
      setError(c('serviceError'));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form onSubmit={submit} className="settings-form">
      <p className="hint">{t('profileHint')}</p>
      <div className="profile-media-preview">
        <Avatar name={user?.username ?? ''} url={user?.avatar_url} size="large" />
        <div>
          <strong>{user?.username}</strong>
          <div className="actions">
            {['avatar', 'banner'].map((kind) => (
              <label className="upload-button" key={kind}>
                <ImagePlus />
                {t(kind)}
                <input
                  type="file"
                  accept="image/png,image/jpeg,image/webp"
                  disabled={busy}
                  onChange={(e) => media(e.target.files?.[0], kind)}
                />
              </label>
            ))}
          </div>
        </div>
      </div>
      {user?.banner_url && (
        <Image
          src={user.banner_url}
          className="profile-banner-preview"
          alt={t('banner')}
          width={1200}
          height={400}
          unoptimized
        />
      )}
      <div className="field">
        <label htmlFor="bio">{t('bio')}</label>
        <Textarea
          id="bio"
          maxLength={4000}
          value={bio}
          onChange={(e) => setBio(e.target.value)}
          rows={6}
        />
      </div>
      <div className="field">
        <label htmlFor="country">{t('country')}</label>
        <Input
          id="country"
          value={country}
          pattern="[A-Z]{2}"
          minLength={2}
          maxLength={2}
          onChange={(e) => setCountry(e.target.value.toUpperCase())}
          className="country-input"
        />
      </div>
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      {feedback && (
        <p role="status" className="form-success">
          {feedback}
        </p>
      )}
      <Button disabled={busy} type="submit">
        {busy && <LoaderCircle className="spin" />}
        {c('save')}
      </Button>
    </form>
  );
}
function PasswordControls() {
  const t = useTranslations('settings');
  const c = useTranslations('common');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [saved, setSaved] = useState(false);
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = e.currentTarget;
    const data = new FormData(form);
    setBusy(true);
    setError(false);
    setSaved(false);
    try {
      await api('/me/password', { method: 'POST', body: JSON.stringify(Object.fromEntries(data)) });
      form.reset();
      setSaved(true);
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="password-form" onSubmit={submit}>
      <h2>{t('password')}</h2>
      <div className="field">
        <label htmlFor="current_password">{t('currentPassword')}</label>
        <Input
          name="current_password"
          id="current_password"
          type="password"
          autoComplete="current-password"
          maxLength={128}
          required
        />
      </div>
      <div className="field">
        <label htmlFor="new_password">{t('newPassword')}</label>
        <Input
          name="new_password"
          id="new_password"
          type="password"
          autoComplete="new-password"
          minLength={12}
          maxLength={128}
          required
        />
        <p className="hint">{t('passwordHint')}</p>
      </div>
      {error && (
        <p className="form-error" role="alert">
          {c('serviceError')}
        </p>
      )}
      {saved && (
        <p className="form-success" role="status">
          {t('passwordChanged')}
        </p>
      )}
      <Button disabled={busy}>
        {busy && <LoaderCircle className="spin" />}
        {t('password')}
      </Button>
    </form>
  );
}
export function SettingsPage() {
  const t = useTranslations('settings');
  const social = useTranslations('social');
  const c = useTranslations('common');
  const current = useLocale();
  const { locale } = useAppearance();
  const { user } = useSession();
  const [localeError, setLocaleError] = useState(false);
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')} />
      <Tabs defaultValue="appearance" className="settings-layout">
        <TabsList className="max-w-full flex-wrap h-auto!">
          <TabsTrigger value="appearance">{t('appearance')}</TabsTrigger>
          <TabsTrigger value="profile">{t('profile')}</TabsTrigger>
          <TabsTrigger value="account">{t('account')}</TabsTrigger>
          <TabsTrigger value="social">{social('title')}</TabsTrigger>
        </TabsList>
        <TabsContent value="appearance">
          <section className="glass settings-panel">
            <AppearanceControls />
          </section>
        </TabsContent>
        <TabsContent value="profile">
          <section className="glass settings-panel">
            <Protected>
              <ProfileControls key={user?.id} />
            </Protected>
          </section>
        </TabsContent>
        <TabsContent value="account">
          <section className="glass settings-panel">
            <h2>{t('accountHint')}</h2>
            {user && (
              <div className="account-details">
                <strong>{user.username}</strong>
                <span>{user.email}</span>
              </div>
            )}
            <EmailVerificationControls key={user?.id} />
            <div className="field">
              <label htmlFor="account-locale">{t('locale')}</label>
              <select
                id="account-locale"
                value={current}
                onChange={(e) => {
                  if (isLocale(e.target.value))
                    locale(e.target.value).catch(() => setLocaleError(true));
                }}
                className="native-select"
              >
                <option value="zh-TW">繁體中文</option>
                <option value="en">English</option>
                <option value="ja">日本語</option>
              </select>
              <p className="hint">{t('localeHint')}</p>
              {localeError && (
                <p className="form-error" role="alert">
                  {c('serviceError')}
                </p>
              )}
            </div>
            <Protected>
              <PasswordControls />
            </Protected>
          </section>
        </TabsContent>
        <TabsContent value="social">
          <section className="glass settings-panel">
            <Protected permission="social.manage">
              <RelationshipList />
            </Protected>
          </section>
        </TabsContent>
      </Tabs>
    </div>
  );
}
