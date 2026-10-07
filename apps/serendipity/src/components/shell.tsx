'use client';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useLocale, useTranslations } from 'next-intl';
import { Disc3, Menu, Palette, ArrowUpRight, LogOut, Sun, Moon } from 'lucide-react';
import { useTheme } from 'next-themes';
import { useState } from 'react';
import { Button } from './ui/button';
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
  const { locale: setLocale, update } = useAppearance();
  const { user, logout } = useSession();
  const { resolvedTheme } = useTheme();
  const [navOpen, setNavOpen] = useState(false);
  const [error, setError] = useState('');
  const links = [
    ['/', 'home'],
    ['/beatmaps', 'beatmaps'],
    ['/rankings', 'rankings'],
    ['/chat', 'community'],
    ['/connect', 'connect'],
    ['/settings', 'settings'],
  ];
  if (user && hasPermission(user.permissions, 'admin.view')) links.push(['/admin', 'admin']);
  const navigation = links.map(([href, key]) => (
    <Link
      key={href}
      href={href}
      className={pathname === href || (href !== '/' && pathname.startsWith(href)) ? 'active' : ''}
      onClick={() => setNavOpen(false)}
    >
      {t(key)}
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
          <nav className="desktop-nav" aria-label={t('menu')}>
            {navigation}
          </nav>
          <div className="header-tools">
            <select
              aria-label={t('language')}
              value={locale}
              onChange={(e) => {
                if (isLocale(e.target.value))
                  setLocale(e.target.value).catch(() => setError(common('serviceError')));
              }}
              className="language-select"
            >
              <option value="zh-TW">繁中</option>
              <option value="en">EN</option>
              <option value="ja">日本語</option>
            </select>
            <Button
              variant="ghost"
              size="icon"
              aria-label={t('appearance')}
              onClick={() => update({ mode: resolvedTheme === 'dark' ? 'light' : 'dark' })}
              className="theme-toggle"
            >
              <Sun className="light-icon" />
              <Moon className="dark-icon" />
            </Button>
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
                <nav className="mobile-nav">
                  {navigation}
                  {user ? (
                    <Link href={`/players/${user.id}`} onClick={() => setNavOpen(false)}>
                      {t('profile')}
                    </Link>
                  ) : (
                    <>
                      <Link href="/login" onClick={() => setNavOpen(false)}>
                        {t('signIn')}
                      </Link>
                      <Link href="/register" onClick={() => setNavOpen(false)}>
                        {t('join')}
                      </Link>
                    </>
                  )}
                </nav>
              </SheetContent>
            </Sheet>
          </div>
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
        <span className="footer-note">{common('legal')}</span>
      </footer>
    </>
  );
}
