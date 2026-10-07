'use client';
import { useState } from 'react';
import Link from 'next/link';
import { useTranslations } from 'next-intl';
import { Copy, Check, ArrowRight, ExternalLink } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { PageHeading } from '@/components/common';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
export function ConnectPage() {
  const t = useTranslations('connect');
  const n = useTranslations('nav');
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  async function copy() {
    setCopyError(false);
    try {
      await navigator.clipboard.writeText(t('stableCommand'));
      setCopied(true);
    } catch {
      setCopied(false);
      setCopyError(true);
    }
  }
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')} />
      <div className="guide-layout">
        <nav className="guide-sidebar" aria-label={t('guide')}>
          {['account', 'client', 'tool', 'tls', 'compatibility', 'restore'].map((id) => (
            <a href={`#${id}`} key={id}>
              {t(`${id}Title`)}
            </a>
          ))}
        </nav>
        <article className="guide-body">
          <section id="account">
            <h2>{t('accountTitle')}</h2>
            <p>{t('accountText')}</p>
            <Button asChild>
              <Link href="/register">
                {n('join')}
                <ArrowRight />
              </Link>
            </Button>
          </section>
          <section id="client">
            <Tabs defaultValue="stable">
              <TabsList>
                <TabsTrigger value="stable">{t('stable')}</TabsTrigger>
                <TabsTrigger value="lazer">{t('lazer')}</TabsTrigger>
              </TabsList>
              <TabsContent value="stable" className="pt-6">
                <h2>{t('stableTitle')}</h2>
                <p>{t('stableText')}</p>
                <div className="command">
                  <code>{t('stableCommand')}</code>
                  <Button variant="ghost" size="icon" aria-label={t('copy')} onClick={copy}>
                    {copied ? <Check /> : <Copy />}
                  </Button>
                </div>
                {copied && (
                  <p className="form-success" role="status">
                    {t('copied')}
                  </p>
                )}
                {copyError && (
                  <p className="form-error" role="alert">
                    {t('copyError')}
                  </p>
                )}
              </TabsContent>
              <TabsContent value="lazer" className="pt-6">
                <h2>{t('lazerTitle')}</h2>
                <p>{t('lazerText')}</p>
                <Link href="/docs/kaleidoscope" className="back-link">
                  {t('download')}
                </Link>
              </TabsContent>
            </Tabs>
          </section>
          <section id="tool">
            <h2>{t('toolTitle')}</h2>
            <p>{t('toolText')}</p>
            <Button asChild>
              <Link href="/docs/kaleidoscope">
                {t('download')}
                <ArrowRight />
              </Link>
            </Button>
            <div className="reference-links">
              <Link href="/docs/deployment">
                {t('operator')}
                <ExternalLink className="inline size-3 ml-1" />
              </Link>
              <Link href="/docs/local-client">
                {t('localTesting')}
                <ExternalLink className="inline size-3 ml-1" />
              </Link>
            </div>
          </section>
          <section id="tls">
            <h2>{t('tlsTitle')}</h2>
            <p>{t('tlsText')}</p>
          </section>
          <section id="compatibility" className="guide-notice">
            <h2>{t('compatibilityTitle')}</h2>
            <p>{t('compatibilityText')}</p>
          </section>
          <section id="restore">
            <h2>{t('restoreTitle')}</h2>
            <p>{t('restoreText')}</p>
          </section>
          <section>
            <h2>{t('source')}</h2>
            <div className="reference-links">
              <a
                href="https://github.com/ppy/osu/wiki/Testing-web-server-full-stack-with-osu!"
                target="_blank"
                rel="noreferrer"
              >
                ppy · lazer full-stack
                <ExternalLink className="inline size-3 ml-1" />
              </a>
              <a
                href="https://github.com/osuAkatsuki/bancho.py/discussions/403"
                target="_blank"
                rel="noreferrer"
              >
                stable · development server
                <ExternalLink className="inline size-3 ml-1" />
              </a>
            </div>
          </section>
        </article>
      </div>
    </div>
  );
}
