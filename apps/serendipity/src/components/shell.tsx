'use client';
import { NavigationLink as Link } from './navigation-motion';
import { usePathname } from 'next/navigation';
import { useLocale, useTranslations } from 'next-intl';
import { Disc3, Menu, Palette, ArrowUpRight, LogOut } from 'lucide-react';
import { LayoutGroup } from 'motion/react';
import * as m from 'motion/react-m';
import { useState } from 'react';
import { Button } from './ui/button';
import { FieldSelect } from './ui/field-select';
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
            <FieldSelect
              aria-label={t('language')}
              value={locale}
              onValueChange={(value) => {
                if (isLocale(value)) setLocale(value).catch(() => setError(common('serviceError')));
              }}
              className="language-select"
              options={[
                { value: 'zh-TW', label: '繁中' },
                { value: 'en', label: 'EN' },
                { value: 'ja', label: '日本語' },
              ]}
            />
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
        <span className="footer-note">{common('legal')}</span>
      </footer>
    </>
  );
}
