'use client';
import { Suspense, useState, type CSSProperties, type FormEvent } from 'react';
import { useSearchParams } from 'next/navigation';
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
  const {
    preferences: p,
    ready,
    loadError,
    reload,
    operation,
    feedback,
    failure,
    update,
    save,
    upload,
    retry,
    reset,
  } = useAppearance();
  const { user } = useSession();
  const busy = operation !== null;
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
              disabled={busy}
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
            disabled={busy}
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
                disabled={busy}
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
          <label className={`upload-button ${!user || busy ? 'disabled' : ''}`}>
            <ImagePlus />
            {t(operation === 'upload' ? 'backgroundUploading' : 'uploadImage')}
            <input
              type="file"
              accept="image/png,image/jpeg,image/webp"
              disabled={!user || busy}
              onChange={(e) => {
                const file = e.target.files?.[0];
                e.target.value = '';
                if (file) void upload(file);
              }}
            />
          </label>
          {p.background_url && (
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => update({ background_url: null })}
            >
              {t('removeBackground')}
            </Button>
          )}
        </div>
      </div>
      {(
        [
          { key: 'background_blur', label: 'blur', max: 48, unit: 'px' },
          { key: 'background_dim', label: 'dim', max: 100, unit: '%' },
          { key: 'panel_blur', label: 'panelBlur', max: 48, unit: 'px' },
          { key: 'panel_opacity', label: 'panelOpacity', max: 100, unit: '%' },
          { key: 'border_radius', label: 'radius', max: 40, unit: 'px' },
        ] as const
      ).map(({ key, label, max, unit }) => (
        <div className="field slider-field" key={key}>
          <div>
            <label htmlFor={`${compact ? 'quick-' : ''}${key}`}>{t(label)}</label>
            <output>
              {unit === '%' ? Math.round(p[key] * 100) : p[key]}
              {unit}
            </output>
          </div>
          <Slider
            id={`${compact ? 'quick-' : ''}${key}`}
            min={0}
            max={max}
            step={1}
            value={[unit === '%' ? Math.round(p[key] * 100) : p[key]]}
            disabled={busy}
            onValueChange={([value]) => update({ [key]: unit === '%' ? value / 100 : value })}
            aria-label={t(label)}
            aria-valuetext={`${unit === '%' ? Math.round(p[key] * 100) : p[key]}${unit}`}
          />
        </div>
      ))}
      <p className="hint">{t('panelHint')}</p>
      <p className="hint">{user ? t('preview') : t('guest')}</p>
      {operation && (
        <p className="hint appearance-pending" role="status">
          <LoaderCircle className="spin" />
          {t(operation === 'save' ? 'appearanceSaving' : 'backgroundUploading')}
        </p>
      )}
      {failure && (
        <div className="appearance-form-error">
          <p className="form-error" role="alert">
            {t(failure.operation === 'save' ? 'appearanceSaveError' : 'backgroundUploadError')}
          </p>
          <Button variant="outline" disabled={busy} onClick={() => void retry()}>
            {c('retry')}
          </Button>
        </div>
      )}
      {feedback && (
        <p className="form-success" role="status">
          <Check />
          {feedback === 'saved'
            ? c('saved')
            : t(feedback === 'uploaded' ? 'imageUploaded' : 'resetDone')}
        </p>
      )}
      <div className="actions">
        <Button onClick={() => void save()} disabled={busy}>
          {operation === 'save' ? <LoaderCircle className="spin" /> : <Check />}
          {operation === 'save' ? t('appearanceSaving') : c('save')}
        </Button>
        <Button
          variant="ghost"
          disabled={busy}
          onClick={reset}
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
  const [accent, setAccent] = useState(
    typeof user?.accent_color === 'string' && /^#[0-9a-f]{6}$/i.test(user.accent_color)
      ? user.accent_color
      : '#f28eb6',
  );
  const validPosition = (value: unknown) =>
    typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 100 ? value : 50;
  const [bannerX, setBannerX] = useState(validPosition(user?.banner_position_x));
  const [bannerY, setBannerY] = useState(validPosition(user?.banner_position_y));
  const draft = {
    bio,
    country,
    accent_color: accent,
    banner_position_x: bannerX,
    banner_position_y: bannerY,
  };
  const [savedDraft, setSavedDraft] = useState(draft);
  const dirty = Object.entries(draft).some(
    ([key, value]) => savedDraft[key as keyof typeof draft] !== value,
  );
  const [pending, setPending] = useState<'save' | 'upload' | 'remove' | null>(null);
  const busy = pending !== null;
  type MediaKind = 'avatar' | 'banner';
  type FailedAction =
    | { action: 'save' }
    | { action: 'upload'; file: File; kind: MediaKind }
    | { action: 'remove'; kind: MediaKind };
  const [failedAction, setFailedAction] = useState<FailedAction | null>(null);
  const [feedback, setFeedback] = useState('');
  const [error, setError] = useState('');
  async function saveProfile() {
    if (busy) return;
    const submitted = { ...draft };
    setPending('save');
    setFailedAction(null);
    setError('');
    setFeedback('');
    try {
      await api('/me/profile', { method: 'PATCH', body: JSON.stringify(submitted) });
      await refresh();
      setSavedDraft(submitted);
      setFeedback(c('saved'));
    } catch {
      setError(t('profileSaveError'));
      setFailedAction({ action: 'save' });
    } finally {
      setPending(null);
    }
  }
  async function submit(e: FormEvent) {
    e.preventDefault();
    await saveProfile();
  }
  async function media(file: File | undefined, kind: MediaKind) {
    if (!file || busy) return;
    setPending('upload');
    setFailedAction(null);
    setError('');
    setFeedback('');
    const data = new FormData();
    data.set('file', file);
    data.set('kind', kind);
    try {
      await api(`/me/media?kind=${kind}`, { method: 'POST', body: data });
      await refresh();
      setFeedback(t('imageUploaded'));
    } catch {
      setError(t('imageUploadError'));
      setFailedAction({ action: 'upload', file, kind });
    } finally {
      setPending(null);
    }
  }
  async function removeMedia(kind: MediaKind) {
    if (busy) return;
    setPending('remove');
    setFailedAction(null);
    setError('');
    setFeedback('');
    try {
      await api('/me/media', { method: 'DELETE', body: JSON.stringify({ kind }) });
      await refresh();
      setFeedback(t('imageRemoved'));
    } catch {
      setError(t('imageRemovalError'));
      setFailedAction({ action: 'remove', kind });
    } finally {
      setPending(null);
    }
  }
  function retry() {
    if (failedAction?.action === 'upload') void media(failedAction.file, failedAction.kind);
    else if (failedAction?.action === 'remove') void removeMedia(failedAction.kind);
  }
  return (
    <form onSubmit={submit} className="settings-form" aria-busy={busy}>
      <p className="hint">{t('profileHint')}</p>
      <div className="field profile-accent-field">
        <label htmlFor="profile-accent">{t('profileAccent')}</label>
        <div className="colour-field">
          <input
            type="color"
            id="profile-accent"
            value={accent}
            disabled={busy}
            aria-describedby="profile-accent-hint"
            onChange={(e) => {
              setAccent(e.target.value);
              setFeedback('');
            }}
          />
          <span>{accent.toUpperCase()}</span>
        </div>
        <p className="hint" id="profile-accent-hint">
          {t('profileAccentHint')}
        </p>
      </div>
      <div
        className="profile-preview profile-accent-scope"
        style={{ '--profile-accent': accent } as CSSProperties}
      >
        {user?.banner_url && (
          <div
            className="profile-banner-preview"
            style={{
              backgroundImage: `url(${JSON.stringify(user.banner_url)})`,
              backgroundPosition: `${bannerX}% ${bannerY}%`,
            }}
            role="img"
            aria-label={t('bannerPreview')}
          />
        )}
        <div className="profile-media-preview">
          <Avatar name={user?.username ?? ''} url={user?.avatar_url} size="large" />
          <div>
            <strong>{user?.username}</strong>
            <p className="profile-accent-example">{t('profileAccentPreview')}</p>
          </div>
        </div>
      </div>
      <div className="actions profile-media-actions">
        {(['avatar', 'banner'] as const).map((kind) => (
          <label className={`upload-button ${busy ? 'disabled' : ''}`} key={kind}>
            <ImagePlus />
            {t(kind)}
            <input
              type="file"
              accept="image/png,image/jpeg,image/webp"
              disabled={busy}
              onChange={(e) => {
                const file = e.target.files?.[0];
                e.target.value = '';
                void media(file, kind);
              }}
            />
          </label>
        ))}
        {user?.avatar_url && (
          <Button
            type="button"
            variant="ghost"
            disabled={busy}
            onClick={() => removeMedia('avatar')}
          >
            {t('removeAvatar')}
          </Button>
        )}
        {user?.banner_url && (
          <Button
            type="button"
            variant="ghost"
            disabled={busy}
            onClick={() => removeMedia('banner')}
          >
            {t('removeBanner')}
          </Button>
        )}
      </div>
      {user?.banner_url ? (
        <fieldset
          className="profile-banner-position"
          disabled={busy}
          aria-describedby="banner-position-hint"
        >
          <legend>{t('bannerPosition')}</legend>
          <p className="hint" id="banner-position-hint">
            {t('bannerPositionHint')}
          </p>
          {(
            [
              { axis: 'x', label: 'bannerPositionX', value: bannerX, update: setBannerX },
              { axis: 'y', label: 'bannerPositionY', value: bannerY, update: setBannerY },
            ] as const
          ).map(({ axis, label, value, update }) => (
            <div className="field slider-field" key={axis}>
              <div>
                <label htmlFor={`banner-position-${axis}`}>{t(label)}</label>
                <output>{value}%</output>
              </div>
              <Slider
                id={`banner-position-${axis}`}
                min={0}
                max={100}
                step={1}
                value={[value]}
                disabled={busy}
                aria-label={t(label)}
                aria-valuetext={`${value}%`}
                onValueChange={([position]) => {
                  update(position);
                  setFeedback('');
                }}
              />
            </div>
          ))}
          <Button
            type="button"
            variant="ghost"
            disabled={busy}
            onClick={() => {
              setBannerX(50);
              setBannerY(50);
              setFeedback('');
            }}
          >
            <RotateCcw />
            {t('centerBanner')}
          </Button>
        </fieldset>
      ) : (
        <p className="hint profile-banner-empty">{t('bannerPositionNoImage')}</p>
      )}
      <div className="field">
        <label htmlFor="bio">{t('bio')}</label>
        <Textarea
          id="bio"
          disabled={pending === 'save'}
          maxLength={4000}
          value={bio}
          onChange={(e) => {
            setBio(e.target.value);
            setFeedback('');
          }}
          rows={6}
        />
      </div>
      <div className="field">
        <label htmlFor="country">{t('country')}</label>
        <Input
          id="country"
          disabled={pending === 'save'}
          value={country}
          pattern="[A-Z]{2}"
          minLength={2}
          maxLength={2}
          onChange={(e) => {
            setCountry(e.target.value.toUpperCase());
            setFeedback('');
          }}
          className="country-input"
        />
      </div>
      {error && (
        <div className="profile-form-error">
          <p role="alert" className="form-error">
            {error}
          </p>
          {failedAction && (
            <Button
              type={failedAction.action === 'save' ? 'submit' : 'button'}
              variant="outline"
              disabled={busy}
              onClick={failedAction.action === 'save' ? undefined : retry}
            >
              {c('retry')}
            </Button>
          )}
        </div>
      )}
      {pending && (
        <p role="status" className="hint profile-pending">
          <LoaderCircle className="spin" />
          {t(
            pending === 'save'
              ? 'profileSaving'
              : pending === 'upload'
                ? 'imageUploading'
                : 'imageRemoving',
          )}
        </p>
      )}
      {dirty && (
        <p role="status" className="hint profile-draft">
          {t('profileDraft')}
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
  return (
    <Suspense fallback={<Loading />}>
      <SettingsContent />
    </Suspense>
  );
}

function SettingsContent() {
  const t = useTranslations('settings');
  const social = useTranslations('social');
  const c = useTranslations('common');
  const current = useLocale();
  const { locale } = useAppearance();
  const { user } = useSession();
  const search = useSearchParams();
  const requestedTab = search.get('tab');
  const tab = ['appearance', 'profile', 'account', 'social'].includes(requestedTab ?? '')
    ? requestedTab!
    : 'appearance';
  const [localeError, setLocaleError] = useState(false);
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')} />
      <Tabs
        value={tab}
        onValueChange={(value) => {
          const query = new URLSearchParams(search.toString());
          query.set('tab', value);
          window.history.pushState(null, '', `?${query}`);
        }}
        className="settings-layout"
      >
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
