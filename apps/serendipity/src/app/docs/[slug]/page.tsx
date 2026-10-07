import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { notFound } from 'next/navigation';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import Link from 'next/link';
import { getTranslations } from 'next-intl/server';
import { documentFilename, documentLink, remarkDocumentHeadings } from '@/lib/docs-navigation';

export default async function Page({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const file = documentFilename(slug);
  if (!file) notFound();
  const t = await getTranslations('common');
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
          remarkPlugins={[remarkGfm, remarkDocumentHeadings]}
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
            a: ({ href, children }) => {
              const destination = documentLink(href);
              if (!destination) return <span>{children}</span>;
              return /^[#?]/.test(destination) || /^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(destination)
                ? <a href={destination}>{children}</a>
                : <Link href={destination}>{children}</Link>;
            },
          }}
        >
          {markdown}
        </Markdown>
      </article>
    </div>
  );
}
