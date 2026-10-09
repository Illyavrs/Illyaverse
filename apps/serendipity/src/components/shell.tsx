'use client';
import { NavigationLink as Link } from './navigation-motion';
import { usePathname } from 'next/navigation';
import { useLocale, useTranslations } from 'next-intl';
import {
  Disc3,
  Menu,
  Palette,
  ArrowUpRight,
  LogOut,
  Globe2,
  ChevronDown,
  Check,
} from 'lucide-react';
import { LayoutGroup } from 'motion/react';
import * as m from 'motion/react-m';
import { useEffect, useRef, useState } from 'react';
import { Button } from './ui/button';
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from './ui/dropdown-menu';
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetDescription,
  SheetTrigger,
} from './ui/sheet';
import { useAppearance, useSession } from './providers';
import { hasPermission } from '@/lib/api';
import { isLocale } from '@/i18n/config';
import { AppearanceControls } from '@/features/settings';
import { Avatar } from './common';

export function Shell({ children }: { children: React.ReactNode }) {
  const t = useTranslations('nav');
  const common = useTranslations('common');
  const settings = useTranslations('settings');
  const pathname = usePathname();
  const locale = useLocale();
  const { locale: setLocale } = useAppearance();
  const { user, logout } = useSession();
  const [navOpen, setNavOpen] = useState(false);
  const [error, setError] = useState('');
  const [localeBusy, setLocaleBusy] = useState(false);
  const localeInFlight = useRef(false);
  const languageMenuRef = useRef<HTMLDivElement>(null);
  const languageTriggerRef = useRef<HTMLButtonElement>(null);
  const restoreLanguageFocus = useRef(false);
  useEffect(() => {
    if (localeBusy || !restoreLanguageFocus.current) return;
    const frame = requestAnimationFrame(() => {
      restoreLanguageFocus.current = false;
      const active = document.activeElement;
      if (active === document.body || languageMenuRef.current?.contains(active))
        languageTriggerRef.current?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [localeBusy, locale]);
  async function chooseLocale(value: string) {
    if (!isLocale(value) || value === locale || localeInFlight.current) return;
    // The menu closes while its trigger is disabled for saving. Restore focus
    // after enabling the current trigger, unless the player focused elsewhere.
    restoreLanguageFocus.current =
      languageMenuRef.current?.contains(document.activeElement) ?? false;
    localeInFlight.current = true;
    setLocaleBusy(true);
    setError('');
    try {
      await setLocale(value);
    } catch {
      setError(common('serviceError'));
    } finally {
      localeInFlight.current = false;
      setLocaleBusy(false);
    }
  }
  const links = [
    ['/', 'home'],
    ['/news', 'news'],
    ['/beatmaps', 'beatmaps'],
    ['/rankings', 'rankings'],
    ['/chat', 'community'],
    ['/connect', 'connect'],
    ['/settings', 'settings'],
  ];
  if (user && hasPermission(user.permissions, 'admin.view')) links.push(['/admin', 'admin']);
  const navigation = () =>
    links.map(([href, key]) => (
      <Link
        key={href}
        href={href}
        className={pathname === href || (href !== '/' && pathname.startsWith(href)) ? 'active' : ''}
        aria-current={
          pathname === href || (href !== '/' && pathname.startsWith(href)) ? 'page' : undefined
        }
        onNavigate={() => setNavOpen(false)}
      >
        {t(key)}
        {(pathname === href || (href !== '/' && pathname.startsWith(href))) && (
          <m.span
            className="navigation-active-indicator"
            layoutId="active-route"
            transition={{ type: 'spring', stiffness: 450, damping: 38 }}
            aria-hidden="true"
          />
        )}
      </Link>
    ));
  return (
    <>
      <a className="skip-link" href="#main">
        {common('skip')}
      </a>
      <header className="site-header">
        <div className="header-inner">
          <Link href="/" className="brand" aria-label="Illyaverse">
            <span className="brand-mark">
              <Disc3 />
            </span>
            <span>
              Illya<span>verse</span>
            </span>
          </Link>
          <div className="header-tools">
            <DropdownMenu
              modal={false}
              onOpenChange={(open) => {
                if (open)
                  requestAnimationFrame(() => {
                    languageMenuRef.current
                      ?.querySelector<HTMLElement>('[aria-checked="true"]')
                      ?.focus();
                  });
              }}
            >
              <DropdownMenuTrigger asChild>
                <Button
                  ref={languageTriggerRef}
                  variant="ghost"
                  className="language-select language-menu-trigger"
                  aria-label={t('language')}
                  data-value={locale}
                  disabled={localeBusy}
                  aria-busy={localeBusy}
                >
                  <Globe2 aria-hidden="true" />
                  <span>{locale === 'zh-TW' ? '繁中' : locale === 'ja' ? 'JA' : 'EN'}</span>
                  <ChevronDown aria-hidden="true" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                ref={languageMenuRef}
                align="end"
                className="language-menu-content"
                aria-label={t('language')}
              >
                {[
                  { value: 'zh-TW', label: '繁體中文' },
                  { value: 'en', label: 'English' },
                  { value: 'ja', label: '日本語' },
                ].map(({ value, label }) => (
                  <DropdownMenuItem
                    key={value}
                    role="menuitemradio"
                    aria-checked={locale === value}
                    data-option-value={value}
                    onSelect={() => void chooseLocale(value)}
                    disabled={localeBusy}
                  >
                    <span lang={value}>{label}</span>
                    {locale === value ? (
                      <Check aria-hidden="true" />
                    ) : (
                      <span className="language-check-space" aria-hidden="true" />
                    )}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <Sheet>
              <SheetTrigger asChild>
                <Button variant="ghost" size="icon" aria-label={t('appearance')}>
                  <Palette />
                </Button>
              </SheetTrigger>
              <SheetContent className="appearance-sheet">
                <SheetHeader>
                  <SheetTitle>{t('appearance')}</SheetTitle>
                  <SheetDescription>{settings('subtitle')}</SheetDescription>
                </SheetHeader>
                <AppearanceControls compact />
              </SheetContent>
            </Sheet>
            {user ? (
              <>
                <Link href={`/players/${user.id}`} className="user-link" aria-label={t('profile')}>
                  <Avatar name={user.username} url={user.avatar_url} />
                  <span>{user.username}</span>
                </Link>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={t('signOut')}
                  onClick={() => logout().catch(() => setError(common('serviceError')))}
                >
                  <LogOut />
                </Button>
              </>
            ) : (
              <Button asChild variant="outline" className="sign-in">
                <Link href="/login">{t('signIn')}</Link>
              </Button>
            )}
            <Sheet open={navOpen} onOpenChange={setNavOpen}>
              <SheetTrigger asChild>
                <Button variant="ghost" size="icon" className="mobile-menu" aria-label={t('menu')}>
                  <Menu />
                </Button>
              </SheetTrigger>
              <SheetContent>
                <SheetHeader>
                  <SheetTitle>Illyaverse</SheetTitle>
                  <SheetDescription>{t('menu')}</SheetDescription>
                </SheetHeader>
                <LayoutGroup id="mobile-navigation">
                  <nav className="mobile-nav">
                    {navigation()}
                    {user ? (
                      <Link href={`/players/${user.id}`} onNavigate={() => setNavOpen(false)}>
                        {t('profile')}
                      </Link>
                    ) : (
                      <>
                        <Link href="/login" onNavigate={() => setNavOpen(false)}>
                          {t('signIn')}
                        </Link>
                        <Link href="/register" onNavigate={() => setNavOpen(false)}>
                          {t('join')}
                        </Link>
                      </>
                    )}
                  </nav>
                </LayoutGroup>
              </SheetContent>
            </Sheet>
          </div>
        </div>
        <div className="site-navigation">
          <LayoutGroup id="desktop-navigation">
            <nav className="desktop-nav" aria-label={t('menu')}>
              {navigation()}
            </nav>
          </LayoutGroup>
        </div>
        {error && (
          <div className="shell-error" role="alert">
            {error}
            <button onClick={() => setError('')}>{common('close')}</button>
          </div>
        )}
      </header>
      <main id="main" className="main-content">
        {children}
      </main>
      <footer className="site-footer">
        <div>
          <Link href="/" className="footer-brand">
            Illyaverse
          </Link>
          <span>{common('footer')}</span>
        </div>
        <nav>
          <Link href="/connect">
            {t('connect')}
            <ArrowUpRight />
          </Link>
          <Link href="/settings">
            {t('settings')}
            <ArrowUpRight />
          </Link>
          <a href="https://github.com/ppy/osu" target="_blank" rel="noreferrer">
            osu!
            <ArrowUpRight />
          </a>
        </nav>
        <span className="footer-note">
          {common('legal')} ·{' '}
          <a className="flag-attribution" href="/flags/twemoji/ATTRIBUTION.txt">
            Twemoji · CC BY 4.0
          </a>
        </span>
      </footer>
    </>
  );
}
