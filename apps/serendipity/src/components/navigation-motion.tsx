'use client';

import Link from 'next/link';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import { LazyMotion, MotionConfig } from 'motion/react';
import type { Middleware } from 'swr';
import {
  createContext,
  Suspense,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useTransition,
  type ComponentProps,
  type ReactNode,
} from 'react';

const loadMotion = () => import('./motion-features').then((module) => module.default);
type NavigationOptions = { replace?: boolean; scroll?: boolean; refresh?: boolean };
type NavigationOperation = {
  destination: string;
  finish: () => void;
  releaseView: () => void;
  view?: ViewTransition;
  fallback?: ReturnType<typeof setTimeout>;
  sawPending: boolean;
};
type NavigationProgress = {
  beginRequest: () => () => void;
  navigate: (href: string, options?: NavigationOptions) => void;
  refresh: () => void;
  commit: (url: string) => void;
};
const NavigationContext = createContext<NavigationProgress | null>(null);

export function useNavigationProgress() {
  const value = useContext(NavigationContext);
  if (!value) throw new Error('NavigationProgressProvider is required');
  return value;
}

export function useNavigationLoading(loading: boolean) {
  const { beginRequest } = useNavigationProgress();
  useEffect(() => {
    if (loading) return beginRequest();
  }, [loading, beginRequest]);
}

// SWR's isLoading describes missing initial data; cached background revalidation
// remains quiet. The effect cleanup also releases requests when a route unmounts.
export const navigationLoadingMiddleware: Middleware = (useSWRNext) =>
  function useSWRWithNavigation(key, fetcher, config) {
    const result = useSWRNext(key, fetcher, config);
    useNavigationLoading(Boolean(result.isLoading));
    return result;
  };

function routeUrl(url: URL) {
  return `${url.pathname}${url.search}`;
}

// Allow React's committed DOM to settle before the browser captures the new view.
// A background tab may not receive animation frames, so it also has a short bound.
function afterCommit(callback: () => void) {
  let completed = false;
  const finish = () => {
    if (completed) return;
    completed = true;
    clearTimeout(bound);
    callback();
  };
  const bound = setTimeout(finish, 120);
  requestAnimationFrame(() => requestAnimationFrame(finish));
}

function NavigationCommitObserver() {
  const pathname = usePathname();
  const search = useSearchParams();
  const { commit } = useNavigationProgress();
  const query = search.toString();
  useLayoutEffect(() => {
    commit(`${pathname}${query ? `?${query}` : ''}`);
  }, [pathname, query, commit]);
  return null;
}

export function NavigationProgressProvider({ children }: { children: ReactNode }) {
  const router = useRouter();
  const t = useTranslations('common');
  const [transitionPending, startTransition] = useTransition();
  const [pendingRequests, setPendingRequests] = useState(0);
  const requests = useRef(new Set<symbol>());
  const operation = useRef<NavigationOperation | null>(null);
  const committedUrl = useRef<string | null>(null);

  const beginRequest = useCallback(() => {
    const token = Symbol('navigation-request');
    requests.current.add(token);
    setPendingRequests(requests.current.size);
    return () => {
      if (requests.current.delete(token)) setPendingRequests(requests.current.size);
    };
  }, []);

  const settle = useCallback((current: NavigationOperation, skip = false) => {
    if (current.fallback) clearTimeout(current.fallback);
    if (skip) current.view?.skipTransition();
    current.releaseView();
    current.finish();
    if (operation.current === current) operation.current = null;
  }, []);

  const commit = useCallback(
    (url: string) => {
      const previous = committedUrl.current;
      committedUrl.current = url;
      const current = operation.current;
      if (current && (current.destination === url || (previous !== null && previous !== url))) {
        afterCommit(() => settle(current));
      }
    },
    [settle],
  );

  const navigate = useCallback(
    (href: string, options: NavigationOptions = {}) => {
      const destination = new URL(href, window.location.href);
      if (destination.origin !== window.location.origin) return;
      if (operation.current) settle(operation.current, true);
      const changesRoute = routeUrl(destination) !== routeUrl(new URL(window.location.href));
      if (!changesRoute && !options.refresh) return;

      let releaseView = () => {};
      const committed = new Promise<void>((resolve) => {
        releaseView = resolve;
      });
      const current: NavigationOperation = {
        destination: routeUrl(destination),
        finish: beginRequest(),
        releaseView,
        sawPending: false,
      };
      operation.current = current;
      const update = () => {
        if (operation.current !== current) return;
        startTransition(() => {
          if (changesRoute) {
            if (options.replace) router.replace(href, { scroll: options.scroll });
            else router.push(href, { scroll: options.scroll });
          }
          if (options.refresh) router.refresh();
        });
      };

      if (
        typeof document.startViewTransition === 'function' &&
        !window.matchMedia('(prefers-reduced-motion: reduce)').matches &&
        document.visibilityState === 'visible'
      ) {
        try {
          current.view = document.startViewTransition(() => {
            update();
            return committed;
          });
          // Native snapshots must never hold the interface while a route is slow.
          // The request indicator continues until the real route settles.
          current.fallback = setTimeout(() => {
            current.view?.skipTransition();
            current.releaseView();
          }, 700);
          void current.view.ready.catch(() => {});
          void current.view.finished.catch(() => {});
          void current.view.updateCallbackDone.catch(() => {});
        } catch {
          update();
        }
      } else update();
    },
    [beginRequest, router, settle],
  );

  const refresh = useCallback(() => {
    if (operation.current) settle(operation.current, true);
    operation.current = {
      destination: routeUrl(new URL(window.location.href)),
      finish: beginRequest(),
      releaseView: () => {},
      sawPending: false,
    };
    startTransition(() => router.refresh());
  }, [beginRequest, router, settle]);

  useEffect(() => {
    const current = operation.current;
    if (!current) return;
    if (transitionPending) current.sawPending = true;
    else if (current.sawPending) afterCommit(() => settle(current));
  }, [transitionPending, settle]);

  useEffect(() => {
    const onPopState = () => {
      if (operation.current) settle(operation.current, true);
      const destination = routeUrl(new URL(window.location.href));
      if (destination === committedUrl.current) return;
      operation.current = {
        destination,
        finish: beginRequest(),
        releaseView: () => {},
        sawPending: false,
      };
    };
    const onPageHide = () => {
      if (operation.current) settle(operation.current, true);
    };
    window.addEventListener('popstate', onPopState);
    window.addEventListener('pagehide', onPageHide);
    return () => {
      window.removeEventListener('popstate', onPopState);
      window.removeEventListener('pagehide', onPageHide);
      if (operation.current) settle(operation.current, true);
    };
  }, [beginRequest, settle]);

  const context = useMemo(
    () => ({ beginRequest, navigate, refresh, commit }),
    [beginRequest, navigate, refresh, commit],
  );
  const pending = pendingRequests > 0 || transitionPending;
  return (
    <MotionConfig reducedMotion="user">
      <LazyMotion features={loadMotion} strict>
        <NavigationContext.Provider value={context}>
          <div
            className="navigation-progress"
            data-pending={pending}
            role="progressbar"
            aria-label={t('loading')}
            aria-hidden={!pending}
          />
          <Suspense fallback={null}>
            <NavigationCommitObserver />
          </Suspense>
          {children}
        </NavigationContext.Provider>
      </LazyMotion>
    </MotionConfig>
  );
}

type NavigationLinkProps = ComponentProps<typeof Link>;
function hrefString(href: NavigationLinkProps['href']) {
  if (typeof href === 'string') return href;
  const query = new URLSearchParams(typeof href.query === 'string' ? href.query : undefined);
  if (typeof href.query !== 'string') {
    Object.entries(href.query ?? {}).forEach(([key, value]) => {
      if (Array.isArray(value)) value.forEach((item) => query.append(key, String(item)));
      else if (value !== null && value !== undefined) query.set(key, String(value));
    });
  }
  const search = href.search ?? (query.size ? `?${query}` : '');
  const hash = href.hash ? `#${href.hash.replace(/^#/, '')}` : '';
  return `${href.pathname ?? ''}${search && !search.startsWith('?') ? '?' : ''}${search}${hash}`;
}

// Keep Next's Link for prefetching, accessible anchors and modifier-key handling.
// onNavigate runs only for the same-origin navigation Next will actually perform.
export function NavigationLink({ onNavigate, ...props }: NavigationLinkProps) {
  const { navigate } = useNavigationProgress();
  return (
    <Link
      {...props}
      onNavigate={(event) => {
        let cancelled = false;
        onNavigate?.({
          preventDefault: () => {
            cancelled = true;
            event.preventDefault();
          },
        });
        if (cancelled) return;
        const href = hrefString(props.as ?? props.href);
        const current = new URL(window.location.href);
        const destination = new URL(href, current);
        if (destination.href === current.href) {
          event.preventDefault();
          return;
        }
        if (routeUrl(destination) === routeUrl(current)) return;
        event.preventDefault();
        navigate(href, { replace: props.replace, scroll: props.scroll });
      }}
    />
  );
}
