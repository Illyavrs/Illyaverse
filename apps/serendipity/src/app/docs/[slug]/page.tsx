import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { notFound } from 'next/navigation';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import Link from 'next/link';
import { getTranslations } from 'next-intl/server';
const documents: Record<string, string> = {
  connection: 'connection.md',
  'local-client': 'local-client-testing.md',
  'lazer-client': 'lazer-client-build.md',
  'native-chat': 'native-chat.md',
  'native-mods': 'native-mods.md',
  'score-normalization': 'score-normalization.md',
  replays: 'lazer-replays.md',
  deployment: 'deployment.md',
  architecture: 'architecture.md',
  protocol: 'protocol-compatibility.md',
  realtime: 'signalr-compatibility.md',
  verification: 'verification.md',
};
const documentLink = (href?: string) => {
  const match = Object.entries(documents).find(([, filename]) => href?.split('#')[0] === filename);
  return match ? `/docs/${match[0]}${href?.includes('#') ? `#${href.split('#')[1]}` : ''}` : href;
};
export default async function Page({ params }: { params: Promise<{ slug: string }> }) {
  const t = await getTranslations('common');
  const { slug } = await params;
  const file = documents[slug];
  if (!file) notFound();
  const root = process.env.ILLYA_DOCS_PATH ?? path.resolve(process.cwd(), '../../docs');
  // The Docker runtime explicitly includes the allowlisted docs directory.
  const markdown = await readFile(
    /* turbopackIgnore: true */ path.join(/* turbopackIgnore: true */ root, file),
    'utf8',
  ).catch(() => null);
  if (!markdown) notFound();
  return (
    <div className="page-container">
      <article className="markdown-document glass">
        <Markdown
          remarkPlugins={[remarkGfm]}
          components={{
            table: ({ children, ...props }) => (
              <div
                className="table-scroll"
                tabIndex={0}
                role="region"
                aria-label={t('tableScroll')}
              >
                <table {...props}>{children}</table>
              </div>
            ),
            a: ({ href, children }) => <Link href={documentLink(href) ?? '#'}>{children}</Link>,
          }}
        >
          {markdown}
        </Markdown>
      </article>
    </div>
  );
}
