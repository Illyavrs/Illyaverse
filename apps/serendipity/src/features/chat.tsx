'use client';

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FormEvent } from 'react';
import Link from 'next/link';
import { useFormatter, useTranslations } from 'next-intl';
import { Send, LoaderCircle, Plus, ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { PageHeading, Loading, LoadError, Empty, Avatar } from '@/components/common';
import { useSession } from '@/components/providers';
import { ApiError, hasPermission } from '@/lib/api';
import type { Message, User } from '@/lib/types';
import { messagesPath, targetKey, useChatData, type ChatTarget } from './chat-data';

type ScrollPosition = { top: number; height: number; atBottom: boolean };
type SendAttempt = { content: string; uuid: string; is_action: boolean };

function errorKey(error: unknown, operation: 'create' | 'send') {
  if (error instanceof ApiError) {
    switch (error.code) {
      case 'recipient_accepts_friends_only':
        return 'errorFriendOnly';
      case 'recipient_blocked':
      case 'blocked':
      case 'user_blocked':
        return 'errorBlocked';
      case 'self_message':
      case 'self_conversation':
      case 'invalid_recipient':
        return 'errorSelf';
      case 'recipient_not_found':
        return 'errorRecipientNotFound';
      case 'message_uuid_conflict':
        return 'errorUuidConflict';
    }
    if (error.status === 401) return 'errorUnauthorized';
    if (error.status === 403) return 'errorNoPermission';
    if (error.status === 404)
      return operation === 'create' ? 'errorRecipientUnavailable' : 'errorConversationUnavailable';
    if (error.status === 429) return 'errorRateLimit';
  }
  return operation === 'create' ? 'conversationError' : 'messageError';
}

export function ChatPage() {
  const { user, loading } = useSession();
  // All private drafts, previews and request state belong to this account only.
  return loading ? <Loading /> : <AccountChat key={user?.id ?? 'guest'} user={user} />;
}

function AccountChat({ user }: { user: User | null }) {
  const { refresh: refreshSession } = useSession();
  const t = useTranslations('chat');
  const common = useTranslations('common');
  const f = useFormatter();
  const [selected, setSelected] = useState<ChatTarget | null>(null);
  const [sidebarExpanded, setSidebarExpanded] = useState(false);
  const chat = useChatData(user?.id, selected, refreshSession);
  const { markRead } = chat;
  const initial = chat.channels.items[0];
  const current = selected ?? (initial ? { kind: 'channel' as const, id: initial.id } : null);
  const key = current ? targetKey(current) : null;
  const channel =
    current?.kind === 'channel'
      ? chat.channels.items.find((item) => item.id === current.id)
      : undefined;
  const conversation =
    current?.kind === 'conversation'
      ? chat.conversations.items.find((item) => item.id === current.id)
      : undefined;
  const name = channel?.name ?? conversation?.peer.username;
  const history = key ? chat.histories[key] : undefined;
  const available = !!channel || !!conversation;
  const denied =
    history?.denied ||
    (!!current && !available && !chat.channels.loading && !chat.conversations.loading);
  const messages = denied ? [] : (history?.items ?? []);
  const lastId = messages.at(-1)?.id;
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const draftsRef = useRef(drafts);
  const [sending, setSending] = useState<Record<string, boolean>>({});
  const [sendErrors, setSendErrors] = useState<Record<string, string | null>>({});
  const attempts = useRef(new Map<string, SendAttempt>());
  const inFlight = useRef(new Set<string>());
  const [username, setUsername] = useState('');
  const usernameRef = useRef(username);
  const [creating, setCreating] = useState(false);
  const creatingRef = useRef(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const alive = useRef(true);
  const draft = key ? (drafts[key] ?? '') : '';
  const sendError = key ? sendErrors[key] : null;
  const busy = !!key && !!sending[key];
  const canSend =
    !!user &&
    hasPermission(user.permissions, 'chat.send') &&
    available &&
    !denied &&
    (current?.kind !== 'conversation' || conversation?.can_send === true);
  const messageList = useRef<HTMLDivElement>(null);
  const sidebarContent = useRef<HTMLDivElement>(null);
  const sidebarToggle = useRef<HTMLButtonElement>(null);
  const creationFeedback = useRef<HTMLParagraphElement>(null);
  const positions = useRef(new Map<string, ScrollPosition>());
  const pendingScroll = useRef<{ key: string; top: number; height: number } | null>(null);
  const previousView = useRef<{ key: string | null; lastId?: number; expanded?: boolean }>({
    key: null,
  });
  const announcedIds = useRef(new Map<string, number>());

  // Persist the initial channel as a target so the hook can load its history.
  useEffect(() => {
    if (!selected && initial)
      queueMicrotask(() => setSelected({ kind: 'channel', id: initial.id }));
  }, [initial, selected]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const markVisibleRead = useCallback(() => {
    if (
      current?.kind !== 'conversation' ||
      denied ||
      !lastId ||
      document.visibilityState !== 'visible'
    )
      return;
    const list = messageList.current;
    if (!list?.getClientRects().length) return;
    if (list && list.scrollHeight - list.scrollTop - list.clientHeight < 40)
      void markRead(current.id, lastId);
  }, [markRead, current?.id, current?.kind, denied, lastId]);

  useLayoutEffect(() => {
    const list = messageList.current;
    if (!list || !key) return;
    const previous = previousView.current;
    const pending = pendingScroll.current;
    if (pending?.key === key) {
      list.scrollTop = pending.top + list.scrollHeight - pending.height;
      pendingScroll.current = null;
    } else if (previous.key !== key || previous.expanded !== sidebarExpanded) {
      list.scrollTop = positions.current.get(key)?.top ?? list.scrollHeight;
    } else if (previous.lastId !== lastId && positions.current.get(key)?.atBottom !== false) {
      list.scrollTop = list.scrollHeight;
    }
    positions.current.set(key, {
      top: list.scrollTop,
      height: list.scrollHeight,
      atBottom: list.scrollHeight - list.scrollTop - list.clientHeight < 40,
    });
    previousView.current = { key, lastId, expanded: sidebarExpanded };
  }, [
    key,
    lastId,
    messages.length,
    history?.olderLoading,
    history?.olderError,
    denied,
    sidebarExpanded,
  ]);

  useLayoutEffect(() => {
    const panel = sidebarContent.current;
    const feedback = creationFeedback.current;
    if (!sidebarExpanded || !createError || !panel || !feedback) return;
    const bottom = feedback.getBoundingClientRect().bottom - panel.getBoundingClientRect().bottom;
    if (bottom > 0) panel.scrollTop += bottom + 8;
  }, [createError, sidebarExpanded]);

  useEffect(() => {
    markVisibleRead();
    window.addEventListener('focus', markVisibleRead);
    document.addEventListener('visibilitychange', markVisibleRead);
    return () => {
      window.removeEventListener('focus', markVisibleRead);
      document.removeEventListener('visibilitychange', markVisibleRead);
    };
  }, [markVisibleRead, sidebarExpanded]);

  useEffect(() => {
    if (!key || !lastId || !history?.loaded || denied) return;
    const previous = announcedIds.current.get(key);
    announcedIds.current.set(key, lastId);
    if (previous === undefined || previous >= lastId) return;
    const latest = history.items.at(-1);
    if (latest && latest.user_id !== user?.id)
      queueMicrotask(() => setAnnouncement(t('newMessage', { name: latest.username })));
  }, [denied, history?.items, history?.loaded, key, lastId, t, user?.id]);

  function preserveScroll(viewKey: string) {
    const list = messageList.current;
    if (list?.dataset.viewKey === viewKey)
      pendingScroll.current = { key: viewKey, top: list.scrollTop, height: list.scrollHeight };
  }
  function select(target: ChatTarget) {
    pendingScroll.current = null;
    setAnnouncement('');
    setSelected(target);
    setSidebarExpanded(false);
    queueMicrotask(() => {
      if (sidebarToggle.current?.getClientRects().length) sidebarToggle.current.focus();
    });
  }
  function changeDraft(content: string) {
    if (!key) return;
    const next = { ...draftsRef.current, [key]: content };
    draftsRef.current = next;
    setDrafts(next);
    if (!inFlight.current.has(key) && attempts.current.get(key)?.content !== content)
      attempts.current.delete(key);
    setSendErrors((previous) => ({ ...previous, [key]: null }));
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    if (!user || creatingRef.current || !username.trim()) return;
    const submitted = username.trim();
    if (submitted.toLocaleLowerCase() === user.username.toLocaleLowerCase()) {
      setCreateError('errorSelf');
      return;
    }
    creatingRef.current = true;
    setCreating(true);
    setCreateError(null);
    try {
      const created = await chat.createConversation(submitted);
      if (!alive.current) return;
      select({ kind: 'conversation', id: created.id });
      if (usernameRef.current.trim() === submitted) {
        usernameRef.current = '';
        setUsername('');
      }
    } catch (error) {
      if (alive.current) setCreateError(errorKey(error, 'create'));
    } finally {
      creatingRef.current = false;
      if (alive.current) setCreating(false);
    }
  }

  async function send(e: FormEvent) {
    e.preventDefault();
    if (!current || !key || !canSend || !draft.trim() || inFlight.current.has(key)) return;
    const view = current;
    const viewKey = key;
    const content = draft;
    let attempt = attempts.current.get(viewKey);
    if (!attempt || attempt.content !== content || attempt.is_action !== false) {
      attempt = { content, is_action: false, uuid: crypto.randomUUID() };
      attempts.current.set(viewKey, attempt);
    }
    const submitted = attempt;
    inFlight.current.add(viewKey);
    setSending((previous) => ({ ...previous, [viewKey]: true }));
    setSendErrors((previous) => ({ ...previous, [viewKey]: null }));
    try {
      const sent = await chat.request<Message>(messagesPath(view), {
        method: 'POST',
        body: JSON.stringify({
          content: submitted.content,
          is_action: submitted.is_action,
          client_uuid: submitted.uuid,
        }),
      });
      if (!alive.current) return;
      chat.append(view, sent);
      attempts.current.delete(viewKey);
      if (draftsRef.current[viewKey] === content) {
        const next = { ...draftsRef.current, [viewKey]: '' };
        draftsRef.current = next;
        setDrafts(next);
      }
      setAnnouncement(t('messageSent'));
      // An accepted POST remains successful even if the following refresh fails.
      void chat.synchronize();
    } catch (error) {
      if (!alive.current) return;
      setSendErrors((previous) => ({ ...previous, [viewKey]: errorKey(error, 'send') }));
      if (
        draftsRef.current[viewKey] !== content ||
        (error instanceof ApiError && error.status < 500 && ![408, 429].includes(error.status))
      )
        attempts.current.delete(viewKey);
      if (error instanceof ApiError && [401, 404].includes(error.status))
        chat.denyHistory(viewKey, error);
      if (error instanceof ApiError && [401, 403, 404].includes(error.status))
        void chat.synchronize();
    } finally {
      inFlight.current.delete(viewKey);
      if (alive.current) setSending((previous) => ({ ...previous, [viewKey]: false }));
    }
  }

  return (
    <div className="page-container chat-page">
      <PageHeading title={t('title')} description={t('subtitle')} />
      <div className="chat-layout glass" data-sidebar-expanded={sidebarExpanded}>
        <aside className="chat-sidebar">
          <div className="chat-sidebar-toggle">
            <Button
              ref={sidebarToggle}
              type="button"
              variant="ghost"
              aria-expanded={sidebarExpanded}
              aria-controls="chat-sidebar-content"
              aria-label={sidebarExpanded ? t('closeSidebar') : t('openSidebar')}
              onClick={() => setSidebarExpanded((expanded) => !expanded)}
            >
              <span className="chat-sidebar-selection">{name ?? t('select')}</span>
              <span className="chat-sidebar-toggle-label">
                {sidebarExpanded ? t('closeSidebar') : t('openSidebar')}
              </span>
              <ChevronDown aria-hidden="true" />
            </Button>
          </div>
          <div ref={sidebarContent} id="chat-sidebar-content" className="chat-sidebar-content">
            <nav className="channel-list" aria-label={t('channels')}>
              <h2>{t('channels')}</h2>
              {chat.channels.loading && !chat.channels.items.length ? <Loading /> : null}
              {chat.channels.error ? <LoadError retry={() => void chat.synchronize()} /> : null}
              {!chat.channels.loading && !chat.channels.error && !chat.channels.items.length ? (
                <p className="chat-sidebar-note">{t('unavailable')}</p>
              ) : null}
              <div className="chat-channel-items">
                {chat.channels.items.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    data-channel-id={item.id}
                    aria-pressed={current?.kind === 'channel' && current.id === item.id}
                    onClick={() => select({ kind: 'channel', id: item.id })}
                  >
                    <span aria-hidden="true">#</span>
                    {item.name.replace(/^#/, '')}
                  </button>
                ))}
              </div>
            </nav>
            {user ? (
              <>
                <nav className="conversation-list" aria-label={t('conversations')}>
                  <h2>{t('conversations')}</h2>
                  {chat.conversations.loading && !chat.conversations.items.length ? (
                    <Loading />
                  ) : null}
                  {chat.conversations.error ? (
                    <LoadError retry={() => void chat.synchronize()} />
                  ) : null}
                  {!chat.conversations.loading &&
                  !chat.conversations.error &&
                  !chat.conversations.items.length ? (
                    <p className="chat-sidebar-note">{t('noConversations')}</p>
                  ) : null}
                  <div className="chat-conversation-items">
                    {chat.conversations.items.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        data-conversation-id={item.id}
                        aria-pressed={current?.kind === 'conversation' && current.id === item.id}
                        aria-label={
                          item.unread_count
                            ? t('conversationUnread', {
                                name: item.peer.username,
                                count: item.unread_count,
                              })
                            : item.peer.username
                        }
                        onClick={() => select({ kind: 'conversation', id: item.id })}
                      >
                        <Avatar name={item.peer.username} url={item.peer.avatar_url} />
                        <span className="conversation-text">
                          <strong>{item.peer.username}</strong>
                          <span className="conversation-preview">
                            {item.last_message?.content ?? t('noMessages')}
                          </span>
                        </span>
                        {item.unread_count > 0 ? (
                          <span className="conversation-unread" aria-hidden="true">
                            {item.unread_count > 99 ? '99+' : item.unread_count}
                          </span>
                        ) : null}
                      </button>
                    ))}
                  </div>
                  {chat.conversations.items.length < chat.conversations.total ? (
                    <Button
                      type="button"
                      variant="ghost"
                      onClick={() => void chat.loadMoreConversations()}
                      disabled={chat.conversations.loading}
                    >
                      {chat.conversations.loading ? <LoaderCircle className="spin" /> : null}
                      {t('moreConversations')}
                    </Button>
                  ) : null}
                </nav>
                <form onSubmit={create} className="chat-new-conversation">
                  <label htmlFor="chat-recipient">{t('newConversation')}</label>
                  <div>
                    <Input
                      id="chat-recipient"
                      name="target_username"
                      value={username}
                      placeholder={t('username')}
                      maxLength={32}
                      autoComplete="off"
                      aria-invalid={!!createError}
                      aria-describedby={createError ? 'chat-create-error' : undefined}
                      onChange={(event) => {
                        usernameRef.current = event.target.value;
                        setUsername(event.target.value);
                        setCreateError(null);
                      }}
                    />
                    <Button
                      type="submit"
                      variant="outline"
                      size="icon"
                      disabled={creating || !username.trim()}
                      aria-label={t('startConversation')}
                      aria-busy={creating}
                    >
                      {creating ? <LoaderCircle className="spin" /> : <Plus />}
                    </Button>
                  </div>
                  {creating ? <p role="status">{t('openingConversation')}</p> : null}
                  {createError ? (
                    <p
                      ref={creationFeedback}
                      id="chat-create-error"
                      className="form-error"
                      role="alert"
                    >
                      {t(createError)}
                    </p>
                  ) : null}
                </form>
                <div className="chat-privacy chat-preferences">
                  <label htmlFor="pm_friends_only">
                    <input
                      id="pm_friends_only"
                      type="checkbox"
                      checked={chat.preferences.value}
                      disabled={!chat.preferences.loaded || chat.preferences.busy}
                      aria-describedby="chat-privacy-hint"
                      onChange={(event) => void chat.savePreferences(event.target.checked)}
                    />
                    <span>{t('friendsOnly')}</span>
                  </label>
                  <p id="chat-privacy-hint">{t('friendsOnlyHint')}</p>
                  {chat.preferences.busy ? <p role="status">{t('savingPrivacy')}</p> : null}
                  {chat.preferences.saved ? <p role="status">{t('privacySaved')}</p> : null}
                  {chat.preferences.error ? (
                    <>
                      <p className="form-error" role="alert">
                        {chat.preferences.loaded ? t('privacyError') : t('privacyLoadError')}
                      </p>
                      <Button
                        type="button"
                        variant="ghost"
                        onClick={() =>
                          void (chat.preferences.loaded
                            ? chat.savePreferences(chat.preferences.value)
                            : chat.loadPreferences())
                        }
                      >
                        {common('retry')}
                      </Button>
                    </>
                  ) : null}
                </div>
              </>
            ) : (
              <p className="chat-sidebar-note chat-private-signin">
                <Link href="/login">{t('privateSignIn')}</Link>
              </p>
            )}
          </div>
        </aside>
        <section className="chat-main" aria-label={name ?? t('select')}>
          <header className="chat-header">
            <div className="chat-heading">
              {conversation ? (
                <Link
                  href={`/players/${conversation.peer.id}`}
                  aria-label={t('viewProfile', { name: conversation.peer.username })}
                >
                  <Avatar name={conversation.peer.username} url={conversation.peer.avatar_url} />
                </Link>
              ) : null}
              <div>
                <h2>
                  {conversation ? (
                    <Link href={`/players/${conversation.peer.id}`}>
                      {conversation.peer.username}
                    </Link>
                  ) : (
                    (channel?.name ?? t('select'))
                  )}
                </h2>
                <p>
                  {channel?.description ??
                    (conversation ? t('privateConversation') : t('selectHint'))}
                </p>
              </div>
            </div>
            <p className={`chat-connection chat-connection-${chat.connection}`} role="status">
              <span aria-hidden="true" />
              {t(`connection.${chat.connection}`)}
            </p>
          </header>
          <div
            ref={messageList}
            data-channel-id={current?.id}
            data-view-key={key ?? undefined}
            className="messages"
            role="log"
            aria-label={name ?? t('title')}
            aria-live="off"
            tabIndex={0}
            onScroll={(event) => {
              if (!key) return;
              const list = event.currentTarget;
              positions.current.set(key, {
                top: list.scrollTop,
                height: list.scrollHeight,
                atBottom: list.scrollHeight - list.scrollTop - list.clientHeight < 40,
              });
              markVisibleRead();
            }}
          >
            {!current ? (
              chat.channels.loading ? (
                <Loading />
              ) : (
                <Empty title={t('select')} description={t('selectHint')} />
              )
            ) : denied ? (
              <Empty
                title={
                  current.kind === 'conversation'
                    ? t('conversationUnavailable')
                    : t('channelUnavailable')
                }
              />
            ) : !history?.loaded && !history?.error ? (
              <Loading />
            ) : (
              <>
                {history?.error ? <LoadError retry={() => void chat.synchronize()} /> : null}
                {history?.loaded && !messages.length && !history.error ? (
                  <Empty title={current.kind === 'conversation' ? t('privateEmpty') : t('empty')} />
                ) : null}
                {history?.olderError ? (
                  <LoadError
                    retry={() => void chat.loadOlder(current, () => preserveScroll(key!))}
                  />
                ) : null}
                {!history?.exhausted &&
                messages.length > 0 &&
                messages.length < (history?.total ?? 0) ? (
                  <Button
                    type="button"
                    variant="ghost"
                    className="chat-older"
                    onClick={() => void chat.loadOlder(current, () => preserveScroll(key!))}
                    disabled={history?.olderLoading}
                    aria-busy={history?.olderLoading}
                  >
                    {history?.olderLoading ? <LoaderCircle className="spin" /> : null}
                    {t('older')}
                  </Button>
                ) : null}
                {messages.map((message) => (
                  <article
                    className={`message${message.is_action ? ' message-action' : ''}`}
                    key={message.id}
                    data-message-id={message.id}
                  >
                    <Link
                      href={`/players/${message.user_id}`}
                      aria-label={t('viewProfile', { name: message.username })}
                    >
                      <Avatar name={message.username} url={message.avatar_url} />
                    </Link>
                    <div>
                      <header className="message-head">
                        <Link href={`/players/${message.user_id}`}>{message.username}</Link>
                        <time
                          dateTime={message.created_at}
                          title={f.dateTime(new Date(message.created_at), {
                            dateStyle: 'medium',
                            timeStyle: 'short',
                          })}
                        >
                          {f.dateTime(new Date(message.created_at), {
                            hour: '2-digit',
                            minute: '2-digit',
                          })}
                        </time>
                        {message.is_action ? (
                          <span className="message-action-label">{t('action')}</span>
                        ) : null}
                      </header>
                      <p>{message.content}</p>
                    </div>
                  </article>
                ))}
              </>
            )}
          </div>
          <p
            className="chat-announcement sr-only"
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            {announcement}
          </p>
          {current?.kind === 'conversation' && chat.readErrors[current.id] && !denied ? (
            <div className="chat-read-error">
              <p className="form-error" role="alert">
                {t('readError')}
              </p>
              <Button
                type="button"
                variant="ghost"
                disabled={!lastId}
                onClick={() => {
                  if (lastId) void chat.markRead(current.id, lastId, true);
                }}
              >
                {common('retry')}
              </Button>
            </div>
          ) : null}
          {sendError ? (
            <p className="form-error chat-error" role="alert">
              {t(sendError)}
            </p>
          ) : null}
          {canSend ? (
            <form onSubmit={send} className="chat-compose" aria-busy={busy}>
              <Textarea
                aria-label={t('placeholder')}
                placeholder={t('placeholder')}
                value={draft}
                onChange={(event) => changeDraft(event.target.value)}
                maxLength={1000}
                rows={1}
                onKeyDown={(event) => {
                  if (
                    event.key === 'Enter' &&
                    !event.shiftKey &&
                    !event.nativeEvent.isComposing &&
                    event.nativeEvent.keyCode !== 229
                  ) {
                    event.preventDefault();
                    event.currentTarget.form?.requestSubmit();
                  }
                }}
              />
              <Button type="submit" disabled={busy || !draft.trim()}>
                {busy ? <LoaderCircle className="spin" /> : <Send />}
                {sendError ? t('retrySend') : t('send')}
              </Button>
            </form>
          ) : (
            <div className="chat-login">
              {!user ? (
                <Button asChild variant="outline">
                  <Link href="/login">{t('join')}</Link>
                </Button>
              ) : (
                <p role="status">
                  {denied
                    ? t('accessUnavailable')
                    : conversation?.send_restriction === 'friend_only'
                      ? t('sendFriendOnly')
                      : !hasPermission(user.permissions, 'chat.send') ||
                          conversation?.send_restriction === 'no_permission'
                        ? t('sendNoPermission')
                        : t('selectHint')}
                </p>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
