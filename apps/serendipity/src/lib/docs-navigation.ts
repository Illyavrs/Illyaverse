import type { Nodes, PhrasingContent, Root } from 'mdast';
import type {} from 'mdast-util-to-hast';

// Only these public guides may supply a filesystem filename to the reader.
export const documents = Object.freeze({
  connection: 'connection.md',
  kaleidoscope: 'kaleidoscope-launcher.md',
  'local-client': 'local-client-testing.md',
  'lazer-client': 'lazer-client-build.md',
  'native-chat': 'native-chat.md',
  'native-mods': 'native-mods.md',
  'native-maps': 'native-beatmap-discovery.md',
  'map-packages': 'local-map-packages.md',
  'official-archives': 'official-beatmap-archives.md',
  'official-search': 'official-map-search.md',
  relax: 'relax.md',
  'score-normalization': 'score-normalization.md',
  replays: 'lazer-replays.md',
  deployment: 'deployment.md',
  'account-recovery': 'account-recovery.md',
  architecture: 'architecture.md',
  permissions: 'permissions.md',
  announcements: 'announcements.md',
  'rank-history': 'rank-history.md',
  'map-discovery': 'map-discovery.md',
  protocol: 'protocol-compatibility.md',
  realtime: 'signalr-compatibility.md',
  'room-scores': 'lazer-room-scores-plan.md',
  verification: 'verification.md',
  'appearance-state': 'appearance-state.md',
  'profile-presentation': 'profile-presentation.md',
  'native-score-submission': 'native-score-submission.md',
  'mutation-authorization': 'mutation-authorization.md',
  'website-chat': 'website-chat.md',
} as const);

export function documentFilename(slug: unknown): string | undefined {
  return typeof slug === 'string' && Object.hasOwn(documents, slug)
    ? documents[slug as keyof typeof documents]
    : undefined;
}

const filenameRoutes = new Map<string, string>(
  Object.entries(documents).map(([slug, filename]) => [filename, `/docs/${slug}`]),
);

/** Resolve public guides without treating repository paths as website routes. */
export function documentLink(href?: string): string | undefined {
  if (!href) return undefined;
  // Leave external addresses and same-document navigation unchanged.
  if (/^[a-z][a-z\d+.-]*:/i.test(href))
    return /^(?:https?|ircs?|mailto|xmpp):/i.test(href) ? href : undefined;
  if (href.startsWith('//') || /^[#?]/.test(href)) return href;

  const suffixStart = href.search(/[?#]/);
  const pathname = suffixStart === -1 ? href : href.slice(0, suffixStart);
  const suffix = suffixStart === -1 ? '' : href.slice(suffixStart);
  const filename = pathname.match(/^(?:\.\/|\.\.\/docs\/|docs\/|\/docs\/)?([a-z\d-]+\.md)$/)?.[1];
  const route = filename ? filenameRoutes.get(filename) : undefined;
  if (route) return `${route}${suffix}`;

  // Already-canonical public guide links are safe too; unknown docs stay citations.
  if (pathname.startsWith('/docs/')) {
    const slug = pathname.slice('/docs/'.length);
    return documentFilename(slug) ? href : undefined;
  }
  // Other absolute website links retain their authored destination. Relative
  // repository/source citations have no public web destination in this reader.
  return pathname.startsWith('/') && !pathname.includes('\\') && !pathname.split('/').includes('..')
    ? href
    : undefined;
}

function headingText(nodes: readonly PhrasingContent[]): string {
  return nodes
    .map((node): string => {
      if (node.type === 'text' || node.type === 'inlineCode') return node.value;
      if ('children' in node) return headingText(node.children);
      if (node.type === 'image' || node.type === 'imageReference') return node.alt ?? '';
      return node.type === 'break' ? ' ' : '';
    })
    .join('');
}

export function headingSlug(text: string): string {
  return (
    text
      .normalize('NFC')
      .trim()
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, '')
      .replace(/\s+/g, '-') || 'section'
  );
}

/** Original remark plugin: each render owns its IDs, including suffix collisions. */
export function remarkDocumentHeadings() {
  return (tree: Root) => {
    const used = new Set<string>();
    function visit(node: Nodes) {
      if (node.type === 'heading') {
        const base = headingSlug(headingText(node.children));
        let id = base;
        for (let suffix = 1; used.has(id); suffix++) id = `${base}-${suffix}`;
        used.add(id);
        node.data = { ...node.data, hProperties: { ...node.data?.hProperties, id } };
      }
      if ('children' in node) node.children.forEach(visit);
    }
    visit(tree);
  };
}
