'use client';

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useEffectEvent,
  useState,
  type ReactNode,
} from 'react';
import { ThemeProvider, useTheme } from 'next-themes';
import { useRouter } from 'next/navigation';
import useSWR, { SWRConfig } from 'swr';
import { api, ApiError, fetcher } from '@/lib/api';
import { isLocale, type Locale } from '@/i18n/config';
import type { Preferences, User } from '@/lib/types';

const defaults: Preferences = {
  mode: 'dark',
  accent_color: '#f28eb6',
  background_url: null,
  background_blur: 0,
  background_dim: 0.76,
  border_radius: 14,
  locale: 'zh-TW',
};
type Session = {
  user: User | null;
  loading: boolean;
  refresh: () => Promise<unknown>;
  logout: () => Promise<void>;
};
const SessionContext = createContext<Session>({
  user: null,
  loading: true,
  refresh: async () => {},
  logout: async () => {},
});
type Appearance = {
  preferences: Preferences;
  ready: boolean;
  loadError: boolean;
  reload: () => void;
  update: (values: Partial<Preferences>) => void;
  save: () => Promise<void>;
  reset: () => void;
  locale: (value: Locale) => Promise<void>;
};
const AppearanceContext = createContext<Appearance>({
  preferences: defaults,
  ready: false,
  loadError: false,
  reload: () => {},
  update: () => {},
  save: async () => {},
  reset: () => {},
  locale: async () => {},
});
export const useSession = () => useContext(SessionContext);
export const useAppearance = () => useContext(AppearanceContext);

function validate(value: Partial<Preferences>): Preferences {
  const number = (n: unknown, min: number, max: number, fallback: number) =>
    typeof n === 'number' && Number.isFinite(n) ? Math.max(min, Math.min(max, n)) : fallback;
  return {
    mode: ['dark', 'light', 'system'].includes(value.mode ?? '') ? value.mode! : defaults.mode,
    accent_color: /^#[0-9a-f]{6}$/i.test(value.accent_color ?? '')
      ? value.accent_color!
      : defaults.accent_color,
    background_url:
      typeof value.background_url === 'string' &&
      /^\/media\/[a-zA-Z0-9/_-]+\.(png|jpe?g|webp)$/i.test(value.background_url)
        ? value.background_url
        : null,
    background_blur: number(value.background_blur, 0, 48, 0),
    background_dim: number(value.background_dim, 0, 1, 0.76),
    border_radius: number(value.border_radius, 0, 40, 14),
    locale: isLocale(value.locale) ? value.locale : 'zh-TW',
  };
}

function AppearanceProvider({ children }: { children: ReactNode }) {
  const { user, loading } = useSession();
  const router = useRouter();
  const { setTheme } = useTheme();
  const [preferences, setPreferences] = useState(defaults);
  const [loadedUserId, setLoadedUserId] = useState<number | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [revision, setRevision] = useState(0);
  const userId = user?.id;
  const applyPreferences = useEffectEvent((value: Partial<Preferences>, syncLocale = false) => {
    const valid = validate(value);
    setPreferences(valid);
    setTheme(valid.mode);
    if (syncLocale && !document.cookie.includes(`illya_locale=${valid.locale}`)) {
      document.cookie = `illya_locale=${valid.locale}; Path=/; Max-Age=31536000; SameSite=Lax`;
      router.refresh();
    }
  });
  useEffect(() => {
    try {
      const raw = localStorage.getItem('illya_appearance');
      if (raw) {
        const cached = validate(JSON.parse(raw));
        queueMicrotask(() => applyPreferences(cached));
      }
    } catch {
      localStorage.removeItem('illya_appearance');
    }
  }, []);
  useEffect(() => {
    let active = true;
    if (userId !== undefined)
      api<Preferences>('/me/preferences')
        .then((p) => {
          if (active) {
            applyPreferences(p, true);
            setLoadedUserId(userId);
            setLoadError(false);
          }
        })
        .catch(() => {
          if (active) setLoadError(true);
        });
    return () => {
      active = false;
    };
  }, [userId, revision]);
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--user-accent', preferences.accent_color);
    const rgb = preferences.accent_color
      .slice(1)
      .match(/.{2}/g)!
      .map((v) => parseInt(v, 16) / 255)
      .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    const luminance = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    root.style.setProperty('--primary-contrast', luminance > 0.179 ? '#111014' : '#ffffff');
    root.style.setProperty('--radius', `${preferences.border_radius}px`);
    root.style.setProperty('--scene-blur', `${preferences.background_blur}px`);
    root.style.setProperty('--scene-dim', String(preferences.background_dim));
    localStorage.setItem('illya_appearance', JSON.stringify(preferences));
  }, [preferences]);
  const update = useCallback(
    (values: Partial<Preferences>) => {
      setPreferences((p) => validate({ ...p, ...values }));
      if (values.mode) setTheme(values.mode);
    },
    [setTheme],
  );
  async function save() {
    if (user) await api('/me/preferences', { method: 'PATCH', body: JSON.stringify(preferences) });
  }
  async function locale(value: Locale) {
    if (user)
      await api('/me/preferences', { method: 'PATCH', body: JSON.stringify({ locale: value }) });
    document.cookie = `illya_locale=${value}; Path=/; Max-Age=31536000; SameSite=Lax`;
    update({ locale: value });
    router.refresh();
  }
  return (
    <AppearanceContext.Provider
      value={{
        preferences,
        ready: !loading && (!user || loadedUserId === user.id),
        loadError,
        reload: () => {
          setLoadError(false);
          setRevision((r) => r + 1);
        },
        update,
        save,
        reset: () => update(defaults),
        locale,
      }}
    >
      <div
        className="global-scene"
        style={{
          backgroundImage: preferences.background_url
            ? `url(${JSON.stringify(preferences.background_url)})`
            : undefined,
        }}
        aria-hidden="true"
      />
      <div className="global-veil" aria-hidden="true" />
      {children}
    </AppearanceContext.Provider>
  );
}

export function Providers({ children }: { children: ReactNode }) {
  const router = useRouter();
  const { data, error, isLoading, mutate } = useSWR<User | null>('/me', fetcher, {
    shouldRetryOnError: false,
    revalidateOnFocus: true,
  });
  const user = error instanceof ApiError || error ? null : (data ?? null);
  async function logout() {
    await api('/auth/logout', { method: 'POST' });
    await mutate(null, { revalidate: false });
    router.push('/');
    router.refresh();
  }
  return (
    <ThemeProvider attribute="class" defaultTheme="dark" enableSystem>
      <SessionContext.Provider
        value={{ user, loading: isLoading, refresh: () => mutate(), logout }}
      >
        <SWRConfig key={user?.id ?? 'guest'} value={{ provider: () => new Map() }}>
          <AppearanceProvider>{children}</AppearanceProvider>
        </SWRConfig>
      </SessionContext.Provider>
    </ThemeProvider>
  );
}
