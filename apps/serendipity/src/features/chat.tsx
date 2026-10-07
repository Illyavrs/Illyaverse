'use client';
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import Link from 'next/link';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR, { useSWRConfig } from 'swr';
import { Send, LoaderCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { PageHeading, Loading, LoadError, Empty, Avatar } from '@/components/common';
import { useSession } from '@/components/providers';
import { ApiError, api, fetcher, hasPermission } from '@/lib/api';
import type { Channel, Message, Page } from '@/lib/types';

const PAGE_SIZE = 100;
type History = { items: Message[]; loading: boolean; error: boolean; exhausted: boolean };
type ScrollPosition = { top: number; height: number; atBottom: boolean };

function mergeMessages(...pages: Message[][]) {
  const unique = new Map(pages.flat().map((message) => [message.id, message]));
  return [...unique.values()].sort(
    (a, b) => Date.parse(a.created_at) - Date.parse(b.created_at) || a.id - b.id,
  );
}

export function ChatPage() {
  const t = useTranslations('chat');
  const nav = useTranslations('nav');
  const common = useTranslations('common');
  const f = useFormatter();
  const { user } = useSession();
  const { mutate: refreshMessages } = useSWRConfig();
  const {
    data: channels,
    error: channelError,
    isLoading,
    mutate: reloadChannels,
  } = useSWR<Page<Channel>>('/channels', fetcher);
  const [selected, setSelected] = useState<number | null>(null);
  const current = channels?.items.find((c) => c.id === selected) ?? channels?.items[0];
  const [histories, setHistories] = useState<Record<number, History>>({});
  const messageKey = current ? `/channels/${current.id}/messages?page_size=${PAGE_SIZE}` : null;
  const {
    data: messages,
    error,
    mutate,
  } = useSWR<Page<Message>>(messageKey, fetcher, {
    refreshInterval: 3000,
    onSuccess: (page, key) => {
      const channel = Number(key.split('/')[2]);
      setHistories((previous) => ({
        ...previous,
        [channel]: {
          items: mergeMessages(previous[channel]?.items ?? [], page.items),
          loading: previous[channel]?.loading ?? false,
          error: previous[channel]?.error ?? false,
          exhausted: previous[channel]?.exhausted ?? false,
        },
      }));
    },
  });
  const [drafts, setDrafts] = useState<Record<number, string>>({});
  const [busy, setBusy] = useState(false);
  const [sendErrors, setSendErrors] = useState<Record<number, boolean>>({});
  const history = current ? histories[current.id] : undefined;
  const draft = current ? (drafts[current.id] ?? '') : '';
  const sendError = current ? sendErrors[current.id] : false;
  const denied = error instanceof ApiError && (error.status === 401 || error.status === 403);
  const allMessages = useMemo(
    () => mergeMessages(history?.items ?? [], messages?.items ?? []),
    [history?.items, messages?.items],
  );
  const lastId = allMessages.at(-1)?.id;
  const messageList = useRef<HTMLDivElement>(null);
  const positions = useRef(new Map<number, ScrollPosition>());
  const pendingScroll = useRef<{ channel: number; top: number; height: number } | null>(null);
  const previousView = useRef<{ channel?: number; lastId?: number }>({});
  const historyRequests = useRef(new Map<number, AbortController>());
  const sendInFlight = useRef(false);

  useEffect(() => {
    const requests = historyRequests.current;
    return () => {
      for (const controller of requests.values()) controller.abort();
    };
  }, []);

  useLayoutEffect(() => {
    const list = messageList.current;
    const channel = current?.id;
    if (!list || channel === undefined) return;
    const previous = previousView.current;
    const pending = pendingScroll.current;
    if (pending?.channel === channel) {
      list.scrollTop = pending.top + list.scrollHeight - pending.height;
      pendingScroll.current = null;
    } else if (previous.channel !== channel) {
      list.scrollTop = positions.current.get(channel)?.top ?? list.scrollHeight;
    } else if (previous.lastId !== lastId && positions.current.get(channel)?.atBottom !== false) {
      list.scrollTop = list.scrollHeight;
    }
    positions.current.set(channel, {
      top: list.scrollTop,
      height: list.scrollHeight,
      atBottom: list.scrollHeight - list.scrollTop - list.clientHeight < 40,
    });
    previousView.current = { channel, lastId };
  }, [current?.id, lastId, allMessages.length, history?.loading, history?.error]);

  function preserveScroll(channel: number) {
    const list = messageList.current;
    if (list?.dataset.channelId === String(channel)) {
      pendingScroll.current = { channel, top: list.scrollTop, height: list.scrollHeight };
    }
  }

  async function loadOlder() {
    if (!current || historyRequests.current.has(current.id) || history?.exhausted) return;
    const channel = current.id;
    const before = allMessages.reduce((oldest, message) => Math.min(oldest, message.id), Infinity);
    if (!Number.isFinite(before)) return;
    const controller = new AbortController();
    historyRequests.current.set(channel, controller);
    preserveScroll(channel);
    setHistories((previous) => ({
      ...previous,
      [channel]: {
        items: mergeMessages(previous[channel]?.items ?? [], allMessages),
        exhausted: previous[channel]?.exhausted ?? false,
        loading: true,
        error: false,
      },
    }));
    try {
      const older = await api<Page<Message>>(
        `/channels/${channel}/messages?before_id=${before}&page_size=${PAGE_SIZE}`,
        { signal: controller.signal },
      );
      preserveScroll(channel);
      setHistories((previous) => ({
        ...previous,
        [channel]: {
          items: mergeMessages(previous[channel]?.items ?? [], older.items),
          exhausted: older.items.length < PAGE_SIZE,
          loading: false,
          error: false,
        },
      }));
    } catch {
      if (!controller.signal.aborted) {
        preserveScroll(channel);
        setHistories((previous) => ({
          ...previous,
          [channel]: { ...previous[channel], loading: false, error: true },
        }));
      }
    } finally {
      historyRequests.current.delete(channel);
    }
  }

  async function send(e: FormEvent) {
    e.preventDefault();
    if (
      sendInFlight.current ||
      !draft.trim() ||
      !current ||
      !messageKey ||
      !user ||
      !hasPermission(user.permissions, 'chat.send')
    ) {
      return;
    }
    const channel = current.id;
    const content = draft;
    sendInFlight.current = true;
    setBusy(true);
    setSendErrors((previous) => ({ ...previous, [channel]: false }));
    try {
      const sent = await api<Message>(`/channels/${channel}/messages`, {
        method: 'POST',
        body: JSON.stringify({ content }),
      });
      setHistories((previous) => ({
        ...previous,
        [channel]: {
          items: mergeMessages(previous[channel]?.items ?? [], allMessages, [sent]),
          loading: previous[channel]?.loading ?? false,
          error: previous[channel]?.error ?? false,
          exhausted: previous[channel]?.exhausted ?? false,
        },
      }));
      setDrafts((previous) =>
        previous[channel] === content ? { ...previous, [channel]: '' } : previous,
      );
      // A refresh failure must not label an accepted POST as failed and invite a duplicate send.
      void refreshMessages(messageKey).catch(() => undefined);
    } catch {
      setSendErrors((previous) => ({ ...previous, [channel]: true }));
    } finally {
      sendInFlight.current = false;
      setBusy(false);
    }
  }
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')} />
      {isLoading ? (
        <Loading />
      ) : channelError ? (
        <LoadError retry={() => reloadChannels()} />
      ) : !channels?.items.length ? (
        <Empty title={t('unavailable')} />
      ) : (
        <div className="chat-layout glass">
          <nav className="channel-list" aria-label={t('channels')}>
            <h2>{t('channels')}</h2>
            {channels.items.map((channel) => (
              <button
                key={channel.id}
                aria-pressed={current?.id === channel.id}
                onClick={() => {
                  setSelected(channel.id);
                }}
              >
                <span>#</span>
                {channel.name.replace(/^#/, '')}
              </button>
            ))}
          </nav>
          <section className="chat-main">
            <header className="chat-header">
              <div>
                <h2>{current?.name}</h2>
                <p>{current?.description}</p>
              </div>
            </header>
            <div
              ref={messageList}
              data-channel-id={current?.id}
              className="messages"
              role="log"
              aria-label={current?.name}
              aria-live="polite"
              onScroll={(event) => {
                if (!current) return;
                const list = event.currentTarget;
                positions.current.set(current.id, {
                  top: list.scrollTop,
                  height: list.scrollHeight,
                  atBottom: list.scrollHeight - list.scrollTop - list.clientHeight < 40,
                });
              }}
            >
              {error && !denied && <LoadError retry={() => mutate()} />}
              {denied ? (
                <Empty title={common('noPermission')} />
              ) : !messages && !error ? (
                <Loading />
              ) : messages && !allMessages.length ? (
                <Empty title={t('empty')} />
              ) : messages ? (
                <>
                  {history?.error && <LoadError retry={() => void loadOlder()} />}
                  {!history?.exhausted && allMessages.length < messages.total && (
                    <Button
                      variant="ghost"
                      onClick={() => void loadOlder()}
                      disabled={history?.loading}
                      aria-busy={history?.loading}
                    >
                      {history?.loading && <LoaderCircle className="spin" />}
                      {t('older')}
                    </Button>
                  )}
                  {allMessages.map((message) => (
                    <article className="message" key={message.id}>
                      <Link href={`/players/${message.user_id}`}>
                        <Avatar name={message.username} url={message.avatar_url} />
                      </Link>
                      <div>
                        <header className="message-head">
                          <Link href={`/players/${message.user_id}`}>{message.username}</Link>
                          <time dateTime={message.created_at}>
                            {f.dateTime(new Date(message.created_at), {
                              hour: '2-digit',
                              minute: '2-digit',
                            })}
                          </time>
                        </header>
                        <p>{message.content}</p>
                      </div>
                    </article>
                  ))}
                </>
              ) : null}
            </div>
            {sendError && (
              <p className="form-error chat-error" role="alert">
                {t('messageError')}
              </p>
            )}
            {user && hasPermission(user.permissions, 'chat.send') ? (
              <form onSubmit={send} className="chat-compose">
                <Textarea
                  aria-label={t('placeholder')}
                  placeholder={t('placeholder')}
                  value={draft}
                  onChange={(e) => {
                    const content = e.target.value;
                    if (current) setDrafts((previous) => ({ ...previous, [current.id]: content }));
                  }}
                  maxLength={1000}
                  rows={1}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                      e.preventDefault();
                      e.currentTarget.form?.requestSubmit();
                    }
                  }}
                />
                <Button type="submit" disabled={busy || !draft.trim()}>
                  {busy ? <LoaderCircle className="spin" /> : <Send />}
                  {t('send')}
                </Button>
              </form>
            ) : (
              <div className="chat-login">
                {user && <p role="status">{common('noPermission')}</p>}
                <Button asChild variant="outline">
                  <Link href={user ? '/settings' : '/login'}>
                    {user ? nav('settings') : t('join')}
                  </Link>
                </Button>
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
