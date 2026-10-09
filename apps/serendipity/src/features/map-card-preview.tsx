'use client';

import { useEffect, useId, useMemo, useRef, useState, type CSSProperties } from 'react';
import { Popover } from 'radix-ui';
import { useTranslations } from 'next-intl';
import { Star, RefreshCw } from 'lucide-react';
import { api, ApiError } from '@/lib/api';
import { useNavigationProgress } from '@/components/navigation-motion';
import { Button } from '@/components/ui/button';
import { difficultyLevel, type DiscoverySet } from './map-discovery';

type PreviewDifficulty = {
  id: number | null;
  official_id?: number | null;
  version: string;
  mode: number;
  stars: number;
};
type Preview = { items: PreviewDifficulty[]; total: number; stored_count: number };
const modes = ['osu!', 'taiko', 'catch', 'mania'];

/** A card preview is read-only. Opening it never imports missing difficulties. */
export function MapCardPreview({ set }: { set: DiscoverySet }) {
  const t = useTranslations('mapDiscovery');
  const headingId = useId();
  const { beginRequest } = useNavigationProgress();
  const [open, setOpen] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const anchorId = set.representative.id;
  const request = useMemo(() => ({ open, anchorId, attempt }), [open, anchorId, attempt]);
  const [result, setResult] = useState<{
    request: typeof request;
    data?: Preview;
    error?: unknown;
  } | null>(null);
  function keepOpen() {
    if (closeTimer.current) clearTimeout(closeTimer.current);
    closeTimer.current = null;
  }
  function leave() {
    keepOpen();
    closeTimer.current = setTimeout(() => setOpen(false), 180);
  }
  useEffect(
    () => () => {
      if (closeTimer.current) clearTimeout(closeTimer.current);
    },
    [],
  );
  useEffect(() => {
    if (!request.open) return;
    const controller = new AbortController();
    const finish = beginRequest();
    api<Preview>(`/beatmaps/${request.anchorId}/set/preview`, { signal: controller.signal })
      .then(
        (value) => {
          if (!controller.signal.aborted) setResult({ request, data: value });
        },
        (cause: unknown) => {
          if (!controller.signal.aborted) setResult({ request, error: cause });
        },
      )
      .finally(() => {
        finish();
      });
    return () => {
      controller.abort();
      finish();
    };
  }, [request, beginRequest]);
  const current = result?.request === request ? result : null;
  const response = current?.data;
  const error = current?.error;
  const loading = open && !current;
  const withdrawn = error instanceof ApiError && [404, 410].includes(error.status);
  const items: PreviewDifficulty[] = response?.items ?? (withdrawn ? [] : set.preview);
  const count = response?.total ?? set.stored_count;
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          className="discovery-difficulty-trigger"
          aria-label={`${t('difficultyPreview')}: ${set.representative.title}. ${t('storedPreviewCount', { count: set.stored_count })}`}
          onPointerEnter={(event) => {
            if (event.pointerType === 'mouse') {
              keepOpen();
              setOpen(true);
            }
          }}
          onPointerLeave={(event) => {
            if (event.pointerType === 'mouse') leave();
          }}
          onFocus={(event) => {
            if (event.currentTarget.matches(':focus-visible')) setOpen(true);
          }}
          onClick={(event) => {
            event.preventDefault();
            keepOpen();
            setOpen(true);
          }}
        >
          <span className="discovery-difficulty-ticks" aria-hidden="true">
            {set.preview.slice(0, 8).map((map) => (
              <span
                className="difficulty-marker"
                key={map.id}
                style={
                  {
                    '--difficulty-color': `var(--difficulty-${difficultyLevel(map.stars)})`,
                  } as CSSProperties
                }
              />
            ))}
          </span>
          <span className="discovery-difficulty-count" aria-hidden="true">
            {response?.total ?? set.stored_count}
          </span>
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="discovery-difficulty-popover"
          data-slot="popover-content"
          data-material="floating"
          align="end"
          sideOffset={6}
          collisionPadding={12}
          aria-labelledby={headingId}
          onPointerEnter={keepOpen}
          onPointerLeave={(event) => {
            if (event.pointerType === 'mouse') leave();
          }}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          <div className="discovery-difficulty-popover-heading">
            <div>
              <h3 id={headingId}>{t('difficultyPreview')}</h3>
              <p aria-live="polite">
                {t(response ? 'fullPreviewCount' : 'storedPreviewCount', { count })}
              </p>
            </div>
          </div>
          {loading && (
            <p className="difficulty-preview-feedback" role="status">
              {t('previewLoading')}
            </p>
          )}
          {error != null && (
            <div className="difficulty-preview-feedback" role="status">
              <p>{t(withdrawn ? 'previewWithdrawn' : 'previewUnavailable')}</p>
              {!withdrawn && (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => setAttempt((value) => value + 1)}
                >
                  <RefreshCw aria-hidden="true" />
                  {t('retry')}
                </Button>
              )}
            </div>
          )}
          <ul className="discovery-difficulty-options">
            {items.map((map, index) => (
              <li
                className="difficulty-chip difficulty-preview-row"
                key={map.official_id ?? map.id ?? index}
                style={
                  {
                    '--difficulty-color': `var(--difficulty-${difficultyLevel(map.stars)})`,
                  } as CSSProperties
                }
              >
                <span className="difficulty-marker" aria-hidden="true" />
                <span className="difficulty-chip-copy">
                  <span className="difficulty-mode">{modes[map.mode] ?? t('unknownMode')}</span>
                  <span className="difficulty-version" title={map.version}>
                    {map.version}
                  </span>
                </span>
                <span className="difficulty-rating">
                  <Star aria-hidden="true" />
                  {map.stars.toFixed(2)}
                </span>
              </li>
            ))}
          </ul>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
