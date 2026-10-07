import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { documentFilename, documentLink, documents, headingSlug, remarkDocumentHeadings } from '../src/lib/docs-navigation.ts';

function render(markdown) {
  return renderToStaticMarkup(createElement(Markdown, {
    remarkPlugins: [remarkGfm, remarkDocumentHeadings],
    components: {
      a: ({ href, children }) => {
        const destination = documentLink(href);
        return destination ? createElement('a', { href: destination }, children) : createElement('span', null, children);
      },
    },
  }, markdown));
}

test('the public reader retains twenty guides and adds only the five approved guides', () => {
  assert.equal(Object.keys(documents).length, 25);
  assert.equal(documentFilename('kaleidoscope'), 'kaleidoscope-launcher.md');
  for (const slug of ['appearance-state', 'profile-presentation', 'native-score-submission', 'mutation-authorization', 'website-chat'])
    assert.equal(documentFilename(slug), `${slug}.md`);
  assert.equal(new Set(Object.values(documents)).size, 25);
  assert.ok(Object.isFrozen(documents));
});

test('unknown, inherited and traversal slugs cannot produce a filesystem filename', () => {
  for (const slug of [undefined, null, {}, 'missing', '__proto__', 'constructor', 'toString', 'hasOwnProperty', '../connection', '..\\connection', '%2e%2e%2fconnection', 'connection.md', 'connection/../../services', '/connection', 'Connection', 'connection\0'])
    assert.equal(documentFilename(slug), undefined, String(slug));
});

test('all approved filename link forms retain the exact query and fragment suffix', () => {
  for (const [slug, filename] of Object.entries(documents)) {
    for (const prefix of ['', './', '../docs/', 'docs/', '/docs/']) {
      for (const suffix of ['', '?lang=zh-TW&view=reader', '#verification', '?lang=ja#lazer-官方圖譜集資訊', '#section?literal=query'])
        assert.equal(documentLink(`${prefix}${filename}${suffix}`), `/docs/${slug}${suffix}`);
    }
    assert.equal(documentLink(`/docs/${slug}?view=reader#heading`), `/docs/${slug}?view=reader#heading`);
  }
});

test('external URLs, canonical website paths and same-document hashes remain authored', () => {
  for (const href of ['https://example.test/docs/connection.md?x=1#section', 'http://localhost:3000', '//example.test/connection.md', 'mailto:reader@example.test', '#lazer-官方圖譜集資訊', '#verification', '?view=reader#verification', '/connect?client=lazer#download'])
    assert.equal(documentLink(href), href);
});

test('unpublished guides and repository citations become readable text, not broken links', () => {
  for (const href of [undefined, '', 'browser-testing.md', '../services/resonance/src/social_session_tests.rs', '../infra/Caddyfile.client-test', '../.impeccable/review/appearance-state-verdict.md', '../tools/transfiguration/README.md', '../../docs/connection.md', './docs/connection.md', 'docs/../connection.md', '/docs/__proto__', '/docs/constructor', '/docs/../connection', '/docs/unknown.md', '/docs/connection.md/../services', '..\\docs\\connection.md'])
    assert.equal(documentLink(href), undefined, String(href));
});

test('actual Markdown headings expose each existing fragment target', () => {
  const headings = [
    ['Native map discovery checkpoint', 'native-map-discovery-checkpoint'],
    ['Reproducible verification', 'reproducible-verification'],
    ['Verification', 'verification'],
    ['Native upload and lazer discovery checkpoint', 'native-upload-and-lazer-discovery-checkpoint'],
    ['Lazer 官方圖譜集資訊', 'lazer-官方圖譜集資訊'],
  ];
  const html = render(headings.map(([heading]) => `## ${heading}`).join('\n\n'));
  for (const [heading, id] of headings) assert.ok(html.includes(`<h2 id="${id}">${heading}</h2>`));
});

test('inline code, emphasis, links and Unicode text contribute their readable heading text', () => {
  const html = render('## **Lazer** `官方圖譜集資訊`\n\n## *Café* & [日本語](https://example.test)\n\n## Ελληνικά Кириллица\n\n## Café');
  assert.ok(html.includes('<h2 id="lazer-官方圖譜集資訊"><strong>Lazer</strong> <code>官方圖譜集資訊</code></h2>'));
  assert.ok(html.includes('<h2 id="café-日本語">'));
  assert.ok(html.includes('<h2 id="ελληνικά-кириллица">'));
  assert.ok(html.includes('<h2 id="café">'));
  assert.equal(headingSlug('   🔒   '), 'section');
  assert.equal(headingSlug('Café'), headingSlug('Café'));
});

test('duplicate headings and authored suffix collisions receive unique deterministic IDs', () => {
  const markdown = '# Topic\n\n## Topic\n\n### Topic-1\n\n## Topic\n\n## Topic-2\n\n## !\n\n## ?\n\n## Section-1\n\n## Café\n\n## Café';
  const html = render(markdown);
  assert.deepEqual([...html.matchAll(/<h[1-6] id="([^"]+)"/g)].map((match) => match[1]),
    ['topic', 'topic-1', 'topic-1-1', 'topic-2', 'topic-2-1', 'section', 'section-1', 'section-1-1', 'café', 'café-1']);
  assert.equal(render(markdown), html, 'a new render starts its own document ID registry');
});

test('the heading plugin leaves raw HTML escaped and does not enable it', () => {
  const html = render('## Safe\n\n<script>alert(1)</script>\n\n<div id="unsafe">Unsafe</div>');
  assert.ok(html.includes('&lt;script&gt;'));
  assert.ok(!html.includes('<script>'));
  assert.ok(!html.includes('<div id="unsafe">'));
});

test('the actual Markdown URL transform and reader resolver keep unsafe schemes unactionable', () => {
  for (const href of ['javascript:alert(1)', 'data:text/html;base64,PHNjcmlwdD4=', 'vbscript:msgbox(1)', 'file:///etc/passwd'])
    assert.equal(documentLink(href), undefined);
  const html = render('[Script](javascript:alert%281%29) [Data](data:text/html;base64,PHNjcmlwdD4=) [Encoded](jav&#x61;script:alert%281%29) [Repo source](../services/resonance/src/social_session_tests.rs) [Official source](https://github.com/ppy/osu)');
  assert.ok(html.includes('<span>Script</span>'));
  assert.ok(html.includes('<span>Data</span>'));
  assert.ok(html.includes('<span>Encoded</span>'));
  assert.ok(html.includes('<span>Repo source</span>'));
  assert.equal((html.match(/<a /g) ?? []).length, 1);
  assert.ok(html.includes('href="https://github.com/ppy/osu"'));
});
