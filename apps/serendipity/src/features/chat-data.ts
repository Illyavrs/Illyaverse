'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { api, ApiError } from '@/lib/api';
import type { Channel, Conversation, Message, Page, Preferences } from '@/lib/types';

export const CHAT_PAGE_SIZE = 100;
const CONVERSATION_PAGE_SIZE = 50;
export type ChatTarget = { kind: 'channel' | 'conversation'; id: number };
export type ConnectionState = 'connecting' | 'live' | 'reconnecting' | 'polling' | 'offline';
export type ChatHistory = {
  items: Message[];
  total: number;
  loaded: boolean;
  loading: boolean;
  olderLoading: boolean;
  error: Error | null;
  olderError: Error | null;
  exhausted: boolean;
  denied: boolean;
};
type Resource<T> = { items: T[]; total: number; loading: boolean; error: Error | null };
export const targetKey = (target: ChatTarget) => `${target.kind}:${target.id}`;
export const messagesPath = (target: ChatTarget) =>
  `/${target.kind === 'channel' ? 'channels' : 'conversations'}/${target.id}/messages`;
const blankHistory = (): ChatHistory => ({
  items: [],
  total: 0,
  loaded: false,
  loading: false,
  olderLoading: false,
  error: null,
  olderError: null,
  exhausted: false,
  denied: false,
});
const failure = (error: unknown): Error =>
  error instanceof Error ? error : new Error('Request failed');
export const isAccessDenied = (error: unknown) =>
  error instanceof ApiError && [401, 403, 404].includes(error.status);

// The server owns the canonical ID; a POST response and its live echo are one message.
export function mergeChatMessages(...pages: Message[][]) {
  const ids = new Map<number, Message>();
  const uuids = new Map<string, number>();
  for (const message of pages.flat()) {
    if (message.client_uuid) {
      const logicalId = `${message.user_id}:${message.client_uuid}`;
      const previousId = uuids.get(logicalId);
      if (previousId !== undefined && previousId !== message.id) ids.delete(previousId);
      uuids.set(logicalId, message.id);
    }
    ids.set(message.id, message);
  }
  return [...ids.values()].sort((a, b) => a.id - b.id);
}

export function useChatData(
  userId: number | undefined,
  target: ChatTarget | null,
  refreshSession: () => Promise<unknown>,
) {
  const [channels, setChannels] = useState<Resource<Channel>>({
    items: [],
    total: 0,
    loading: true,
    error: null,
  });
  const [conversations, setConversations] = useState<Resource<Conversation>>({
    items: [],
    total: 0,
    loading: userId !== undefined,
    error: null,
  });
  const [histories, setHistories] = useState<Record<string, ChatHistory>>({});
  const [connection, setConnection] = useState<ConnectionState>(
    userId === undefined ? 'polling' : 'connecting',
  );
  const [readErrors, setReadErrors] = useState<Record<number, Error | null>>({});
  const [preferences, setPreferences] = useState({
    loaded: false,
    value: false,
    busy: false,
    saved: false,
    error: null as Error | null,
  });
  const alive = useRef(true);
  const historiesRef = useRef(histories);
  const channelsRef = useRef(channels);
  const conversationsRef = useRef(conversations);
  const currentTarget = useRef<ChatTarget | null>(null);
  const requests = useRef(new Set<AbortController>());
  const latestRequests = useRef(new Map<string, AbortController>());
  const olderRequests = useRef(new Map<string, AbortController>());
  const conversationPages = useRef(1);
  const conversationsLoadingMore = useRef(false);
  const synchronization = useRef(false);
  const synchronizationQueued = useRef(false);
  const readInFlight = useRef(new Set<number>());
  const readThrough = useRef(new Map<number, number>());
  const queuedRead = useRef(new Map<number, number>());
  const preferenceBusy = useRef(false);
  const sessionRefresh = useRef(refreshSession);

  useEffect(() => {
    sessionRefresh.current = refreshSession;
  }, [refreshSession]);

  const updateHistories = useCallback(
    (update: (previous: Record<string, ChatHistory>) => Record<string, ChatHistory>) => {
      if (!alive.current) return;
      const next = update(historiesRef.current);
      historiesRef.current = next;
      setHistories(next);
    },
    [],
  );
  const updateChannels = useCallback((next: Resource<Channel>) => {
    if (!alive.current) return;
    channelsRef.current = next;
    setChannels(next);
  }, []);
  const updateConversations = useCallback((next: Resource<Conversation>) => {
    if (!alive.current) return;
    conversationsRef.current = next;
    setConversations(next);
  }, []);
  const request = useCallback(async <T>(path: string, options: RequestInit = {}) => {
    const controller = new AbortController();
    requests.current.add(controller);
    try {
      return await api<T>(path, { ...options, signal: controller.signal });
    } finally {
      requests.current.delete(controller);
    }
  }, []);
  const denyHistory = useCallback(
    (key: string, error: Error) => {
      updateHistories((previous) => ({
        ...previous,
        [key]: { ...blankHistory(), loaded: true, denied: true, error },
      }));
      if (key.startsWith('conversation:')) {
        const id = Number(key.split(':')[1]);
        updateConversations({
          ...conversationsRef.current,
          items: conversationsRef.current.items.filter((conversation) => conversation.id !== id),
        });
      }
    },
    [updateConversations, updateHistories],
  );
  const denyPrivate = useCallback(
    (error: Error) => {
      updateConversations({ items: [], total: 0, loading: false, error });
      updateHistories((previous) =>
        Object.fromEntries(
          Object.entries(previous).map(([key, history]) => [
            key,
            key.startsWith('conversation:')
              ? { ...blankHistory(), loaded: true, denied: true, error }
              : history,
          ]),
        ),
      );
    },
    [updateConversations, updateHistories],
  );

  const loadLatest = useCallback(
    async (view: ChatTarget) => {
      const key = targetKey(view);
      if (latestRequests.current.has(key)) return;
      const controller = new AbortController();
      latestRequests.current.set(key, controller);
      updateHistories((previous) => ({
        ...previous,
        [key]: { ...(previous[key] ?? blankHistory()), loading: true },
      }));
      try {
        const page = await api<Page<Message>>(`${messagesPath(view)}?page_size=${CHAT_PAGE_SIZE}`, {
          signal: controller.signal,
        });
        if (!alive.current || controller.signal.aborted) return;
        updateHistories((previous) => {
          const old = previous[key] ?? blankHistory();
          return {
            ...previous,
            [key]: {
              ...old,
              items: mergeChatMessages(old.items, page.items),
              total: page.total,
              loaded: true,
              loading: false,
              error: null,
              denied: false,
              exhausted: old.exhausted || page.items.length < CHAT_PAGE_SIZE,
            },
          };
        });
      } catch (error) {
        if (!alive.current || controller.signal.aborted) return;
        if (isAccessDenied(error)) {
          denyHistory(key, failure(error));
          if (error instanceof ApiError && error.status === 401) denyPrivate(error);
        } else
          updateHistories((previous) => ({
            ...previous,
            [key]: { ...(previous[key] ?? blankHistory()), loading: false, error: failure(error) },
          }));
      } finally {
        if (latestRequests.current.get(key) === controller) latestRequests.current.delete(key);
      }
    },
    [denyHistory, denyPrivate, updateHistories],
  );

  const synchronize = useCallback(
    async function synchronizeData() {
      if (!alive.current) return;
      if (synchronization.current) {
        synchronizationQueued.current = true;
        return;
      }
      synchronization.current = true;
      try {
        const cachedIds = new Set(
          Object.values(historiesRef.current).flatMap((history) =>
            history.items.map((message) => message.id),
          ),
        );
        // Sidebar previews are cached private content too and must obey visibility changes.
        for (const conversation of conversationsRef.current.items) {
          if (conversation.last_message) cachedIds.add(conversation.last_message.id);
        }
        const ids = [...cachedIds];
        const visibility = async () => {
          const readable = new Set<number>();
          for (let offset = 0; offset < ids.length; offset += 256) {
            const result = await request<{ message_ids: number[] }>('/chat/visibility', {
              method: 'POST',
              body: JSON.stringify({ message_ids: ids.slice(offset, offset + 256) }),
            });
            result.message_ids.forEach((id) => readable.add(id));
          }
          return readable;
        };
        const [channelResult, conversationResult, visibilityResult] = await Promise.allSettled([
          request<Page<Channel>>('/channels'),
          userId === undefined
            ? Promise.resolve(null)
            : Promise.all(
                Array.from({ length: conversationPages.current }, (_, index) =>
                  request<Page<Conversation>>(
                    `/conversations?page_size=${CONVERSATION_PAGE_SIZE}&page=${index + 1}`,
                  ),
                ),
              ),
          visibility(),
        ]);
        if (!alive.current) return;
        if (channelResult.status === 'fulfilled') {
          const page = channelResult.value;
          updateChannels({ items: page.items, total: page.total, loading: false, error: null });
          const accessible = new Set(
            page.items.map((channel) => targetKey({ kind: 'channel', id: channel.id })),
          );
          updateHistories((previous) =>
            Object.fromEntries(
              Object.entries(previous).map(([key, history]) => [
                key,
                key.startsWith('channel:') && !accessible.has(key)
                  ? { ...blankHistory(), loaded: true, denied: true }
                  : history,
              ]),
            ),
          );
        } else {
          const error = failure(channelResult.reason);
          updateChannels({
            ...channelsRef.current,
            items: isAccessDenied(error) ? [] : channelsRef.current.items,
            loading: false,
            error,
          });
          if (isAccessDenied(error))
            updateHistories((previous) =>
              Object.fromEntries(
                Object.keys(previous).map((key) => [
                  key,
                  { ...blankHistory(), loaded: true, denied: true, error },
                ]),
              ),
            );
        }
        if (conversationResult.status === 'fulfilled' && conversationResult.value) {
          const pages = conversationResult.value;
          const items = [
            ...new Map(
              pages
                .flatMap((page) => page.items)
                .map((conversation) => [conversation.id, conversation]),
            ).values(),
          ];
          updateConversations({ items, total: pages[0]?.total ?? 0, loading: false, error: null });
          const accessible = new Set(
            items.map((conversation) => targetKey({ kind: 'conversation', id: conversation.id })),
          );
          updateHistories((previous) =>
            Object.fromEntries(
              Object.entries(previous).map(([key, history]) => [
                key,
                key.startsWith('conversation:') && !accessible.has(key)
                  ? { ...blankHistory(), loaded: true, denied: true }
                  : history,
              ]),
            ),
          );
        } else if (conversationResult.status === 'rejected') {
          const error = failure(conversationResult.reason);
          if (isAccessDenied(error)) denyPrivate(error);
          else updateConversations({ ...conversationsRef.current, loading: false, error });
        }
        if (visibilityResult.status === 'fulfilled') {
          const readable = visibilityResult.value;
          updateHistories((previous) =>
            Object.fromEntries(
              Object.entries(previous).map(([key, history]) => [
                key,
                {
                  ...history,
                  items: history.items.filter(
                    (message) => !cachedIds.has(message.id) || readable.has(message.id),
                  ),
                },
              ]),
            ),
          );
          updateConversations({
            ...conversationsRef.current,
            items: conversationsRef.current.items.map((conversation) => ({
              ...conversation,
              last_message:
                conversation.last_message &&
                cachedIds.has(conversation.last_message.id) &&
                !readable.has(conversation.last_message.id)
                  ? null
                  : conversation.last_message,
            })),
          });
        } else {
          // A failed authorization check cannot keep previously readable history on screen.
          const error = failure(visibilityResult.reason);
          updateHistories((previous) =>
            Object.fromEntries(
              Object.entries(previous).map(([key, history]) => [
                key,
                {
                  ...history,
                  items: history.items.filter((message) => !cachedIds.has(message.id)),
                  error,
                  denied: isAccessDenied(error),
                  loaded: true,
                  loading: false,
                },
              ]),
            ),
          );
          updateConversations({
            ...conversationsRef.current,
            items: conversationsRef.current.items.map((conversation) => ({
              ...conversation,
              last_message: null,
            })),
          });
          if (isAccessDenied(error)) denyPrivate(error);
        }
        const active = currentTarget.current;
        if (active) {
          const accessible =
            active.kind === 'channel'
              ? channelsRef.current.items.some((channel) => channel.id === active.id)
              : conversationsRef.current.items.some(
                  (conversation) => conversation.id === active.id,
                );
          if (
            accessible &&
            !(visibilityResult.status === 'rejected' && isAccessDenied(visibilityResult.reason))
          )
            await loadLatest(active);
        }
      } finally {
        synchronization.current = false;
        if (synchronizationQueued.current && alive.current) {
          synchronizationQueued.current = false;
          queueMicrotask(() => void synchronizeData());
        }
      }
    },
    [
      denyPrivate,
      loadLatest,
      request,
      updateChannels,
      updateConversations,
      updateHistories,
      userId,
    ],
  );

  const key = target ? targetKey(target) : null;
  useEffect(() => {
    const latest = latestRequests.current;
    const older = olderRequests.current;
    const active = key
      ? { kind: key.split(':')[0] as ChatTarget['kind'], id: Number(key.split(':')[1]) }
      : null;
    currentTarget.current = active;
    for (const [otherKey, controller] of older) {
      if (otherKey !== key) {
        controller.abort();
        older.delete(otherKey);
        updateHistories((previous) => ({
          ...previous,
          [otherKey]: { ...previous[otherKey], olderLoading: false },
        }));
      }
    }
    if (active) void loadLatest(active);
    return () => {
      if (key) {
        latest.get(key)?.abort();
        latest.delete(key);
        older.get(key)?.abort();
      }
    };
  }, [key, loadLatest, updateHistories]);

  const loadOlder = useCallback(
    async (view: ChatTarget, preserveScroll: () => void) => {
      const key = targetKey(view);
      const history = historiesRef.current[key];
      if (!history || history.denied || history.exhausted || olderRequests.current.has(key)) return;
      const before = history.items.reduce(
        (oldest, message) => Math.min(oldest, message.id),
        Infinity,
      );
      if (!Number.isFinite(before)) return;
      const controller = new AbortController();
      olderRequests.current.set(key, controller);
      preserveScroll();
      updateHistories((previous) => ({
        ...previous,
        [key]: { ...previous[key], olderLoading: true, olderError: null },
      }));
      try {
        const page = await api<Page<Message>>(
          `${messagesPath(view)}?page_size=${CHAT_PAGE_SIZE}&before_id=${before}`,
          { signal: controller.signal },
        );
        if (
          !alive.current ||
          controller.signal.aborted ||
          !currentTarget.current ||
          targetKey(currentTarget.current) !== key
        )
          return;
        preserveScroll();
        updateHistories((previous) => ({
          ...previous,
          [key]: {
            ...previous[key],
            items: mergeChatMessages(page.items, previous[key]?.items ?? []),
            olderLoading: false,
            olderError: null,
            exhausted: page.items.length < CHAT_PAGE_SIZE,
          },
        }));
      } catch (error) {
        if (!alive.current || controller.signal.aborted) return;
        if (isAccessDenied(error)) denyHistory(key, failure(error));
        else {
          preserveScroll();
          updateHistories((previous) => ({
            ...previous,
            [key]: { ...previous[key], olderLoading: false, olderError: failure(error) },
          }));
        }
      } finally {
        if (olderRequests.current.get(key) === controller) olderRequests.current.delete(key);
        if (controller.signal.aborted)
          updateHistories((previous) => ({
            ...previous,
            [key]: { ...previous[key], olderLoading: false },
          }));
      }
    },
    [denyHistory, updateHistories],
  );

  const append = useCallback(
    (view: ChatTarget, message: Message) => {
      const key = targetKey(view);
      updateHistories((previous) => {
        const history = previous[key] ?? blankHistory();
        const items = mergeChatMessages(history.items, [message]);
        return {
          ...previous,
          [key]: { ...history, items, total: Math.max(history.total, items.length), loaded: true },
        };
      });
    },
    [updateHistories],
  );

  const createConversation = useCallback(
    async (username: string) => {
      const conversation = await request<Conversation>('/conversations', {
        method: 'POST',
        body: JSON.stringify({ target_username: username.trim() }),
      });
      if (alive.current)
        updateConversations({
          ...conversationsRef.current,
          items: [
            conversation,
            ...conversationsRef.current.items.filter((item) => item.id !== conversation.id),
          ],
          total: Math.max(
            conversationsRef.current.total,
            conversationsRef.current.items.length +
              (conversationsRef.current.items.some((item) => item.id === conversation.id) ? 0 : 1),
          ),
          loading: false,
        });
      return conversation;
    },
    [request, updateConversations],
  );
  const loadMoreConversations = useCallback(async () => {
    if (
      conversationsLoadingMore.current ||
      conversationsRef.current.items.length >= conversationsRef.current.total
    )
      return;
    conversationsLoadingMore.current = true;
    updateConversations({ ...conversationsRef.current, loading: true, error: null });
    try {
      const page = await request<Page<Conversation>>(
        `/conversations?page_size=${CONVERSATION_PAGE_SIZE}&page=${conversationPages.current + 1}`,
      );
      conversationPages.current += 1;
      updateConversations({
        items: [
          ...new Map(
            [...conversationsRef.current.items, ...page.items].map((conversation) => [
              conversation.id,
              conversation,
            ]),
          ).values(),
        ],
        total: page.total,
        loading: false,
        error: null,
      });
    } catch (error) {
      if (isAccessDenied(error)) denyPrivate(failure(error));
      else
        updateConversations({ ...conversationsRef.current, loading: false, error: failure(error) });
    } finally {
      conversationsLoadingMore.current = false;
    }
  }, [denyPrivate, request, updateConversations]);

  const markRead = useCallback(
    async function acknowledge(conversationId: number, messageId: number, retry = false) {
      if (readInFlight.current.has(conversationId)) {
        queuedRead.current.set(
          conversationId,
          Math.max(queuedRead.current.get(conversationId) ?? 0, messageId),
        );
        return;
      }
      if (!retry && (readThrough.current.get(conversationId) ?? 0) >= messageId) return;
      readInFlight.current.add(conversationId);
      readThrough.current.set(conversationId, messageId);
      if (alive.current) setReadErrors((previous) => ({ ...previous, [conversationId]: null }));
      try {
        await request<void | Conversation>(`/conversations/${conversationId}/read`, {
          method: 'PUT',
          body: JSON.stringify({ message_id: messageId }),
        });
        updateConversations({
          ...conversationsRef.current,
          items: conversationsRef.current.items.map((conversation) =>
            conversation.id === conversationId
              ? {
                  ...conversation,
                  last_read_id: Math.max(conversation.last_read_id ?? 0, messageId),
                  unread_count:
                    !conversation.last_message || conversation.last_message.id <= messageId
                      ? 0
                      : conversation.unread_count,
                }
              : conversation,
          ),
        });
      } catch (error) {
        if (!alive.current) return;
        setReadErrors((previous) => ({ ...previous, [conversationId]: failure(error) }));
        if (isAccessDenied(error)) {
          denyHistory(targetKey({ kind: 'conversation', id: conversationId }), failure(error));
          if (error instanceof ApiError && error.status === 401) denyPrivate(error);
        }
      } finally {
        readInFlight.current.delete(conversationId);
        const queued = queuedRead.current.get(conversationId);
        queuedRead.current.delete(conversationId);
        if (
          alive.current &&
          queued &&
          queued > messageId &&
          conversationsRef.current.items.some((conversation) => conversation.id === conversationId)
        )
          void acknowledge(conversationId, queued);
      }
    },
    [denyHistory, denyPrivate, request, updateConversations],
  );

  const loadPreferences = useCallback(async () => {
    try {
      const value = await request<Preferences>('/me/preferences');
      if (alive.current)
        setPreferences({
          loaded: true,
          value: value.pm_friends_only ?? false,
          busy: false,
          saved: false,
          error: null,
        });
    } catch (error) {
      if (alive.current)
        setPreferences((previous) => ({ ...previous, loaded: false, error: failure(error) }));
    }
  }, [request]);
  const savePreferences = useCallback(
    async (value: boolean) => {
      if (preferenceBusy.current) return;
      preferenceBusy.current = true;
      setPreferences({ loaded: true, value, busy: true, saved: false, error: null });
      try {
        await request('/me/preferences', {
          method: 'PATCH',
          body: JSON.stringify({ pm_friends_only: value }),
        });
        if (alive.current)
          setPreferences({ loaded: true, value, busy: false, saved: true, error: null });
      } catch (error) {
        if (alive.current)
          setPreferences({ loaded: true, value, busy: false, saved: false, error: failure(error) });
      } finally {
        preferenceBusy.current = false;
      }
    },
    [request],
  );

  useEffect(() => {
    const pendingRequests = requests.current;
    const pendingLatest = latestRequests.current;
    const pendingOlder = olderRequests.current;
    alive.current = true;
    void synchronize();
    if (userId !== undefined) void loadPreferences();
    let socket: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    let attempts = 0;
    let lastFrame = Date.now();
    const refreshAccount = () => {
      if (userId !== undefined) void sessionRefresh.current().catch(() => undefined);
    };
    const connect = () => {
      if (stopped) return;
      if (!navigator.onLine) {
        setConnection('offline');
        return;
      }
      setConnection(attempts ? 'reconnecting' : 'connecting');
      const url = new URL('/api/v1/chat/stream', window.location.href);
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
      const connectionSocket = new WebSocket(url);
      socket = connectionSocket;
      lastFrame = Date.now();
      connectionSocket.onmessage = (event) => {
        if (stopped || socket !== connectionSocket) return;
        lastFrame = Date.now();
        let data: { type?: string; items?: Message[] };
        try {
          data = JSON.parse(String(event.data));
        } catch {
          return;
        }
        if (data.type === 'ready') {
          attempts = 0;
          setConnection('live');
          refreshAccount();
          void synchronize();
        } else if (data.type === 'refresh') {
          refreshAccount();
          void synchronize();
        } else if (data.type === 'messages' && Array.isArray(data.items)) {
          for (const message of data.items) {
            const kind = channelsRef.current.items.some(
              (channel) => channel.id === message.channel_id,
            )
              ? 'channel'
              : conversationsRef.current.items.some(
                    (conversation) => conversation.id === message.channel_id,
                  )
                ? 'conversation'
                : null;
            if (kind) append({ kind, id: message.channel_id }, message);
          }
          void synchronize();
        }
      };
      connectionSocket.onclose = () => {
        if (stopped || socket !== connectionSocket) return;
        setConnection(navigator.onLine ? 'reconnecting' : 'offline');
        refreshAccount();
        void synchronize();
        const delay = [1000, 2000, 5000, 10000, 30000][Math.min(attempts++, 4)];
        reconnectTimer = setTimeout(connect, delay);
      };
      connectionSocket.onerror = () => connectionSocket.close();
    };
    if (userId !== undefined) connect();
    const interval = setInterval(
      () => {
        if (userId === undefined) setConnection(navigator.onLine ? 'polling' : 'offline');
        if (!socket || socket.readyState !== WebSocket.OPEN) {
          refreshAccount();
          void synchronize();
        }
        // Periodic refresh frames are the server's heartbeat; a stalled socket is not live.
        else if (Date.now() - lastFrame > 30000) socket.close();
      },
      userId === undefined ? 3000 : 10000,
    );
    const onOnline = () => {
      refreshAccount();
      void synchronize();
      if (userId !== undefined && (!socket || socket.readyState === WebSocket.CLOSED)) {
        clearTimeout(reconnectTimer);
        connect();
      }
      if (userId === undefined) setConnection('polling');
    };
    const onOffline = () => {
      setConnection('offline');
      socket?.close();
    };
    const onFocus = () => {
      void synchronize();
    };
    window.addEventListener('online', onOnline);
    window.addEventListener('offline', onOffline);
    window.addEventListener('focus', onFocus);
    return () => {
      alive.current = false;
      stopped = true;
      socket?.close();
      clearTimeout(reconnectTimer);
      clearInterval(interval);
      window.removeEventListener('online', onOnline);
      window.removeEventListener('offline', onOffline);
      window.removeEventListener('focus', onFocus);
      for (const controller of pendingRequests) controller.abort();
      for (const controller of pendingLatest.values()) controller.abort();
      for (const controller of pendingOlder.values()) controller.abort();
    };
  }, [append, loadPreferences, synchronize, userId]);

  return {
    channels,
    conversations,
    histories,
    connection,
    readErrors,
    preferences,
    synchronize,
    loadLatest,
    loadOlder,
    append,
    createConversation,
    loadMoreConversations,
    markRead,
    loadPreferences,
    savePreferences,
    denyHistory,
    request,
  };
}
