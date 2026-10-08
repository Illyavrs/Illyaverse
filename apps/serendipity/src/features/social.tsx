'use client';
import Link from 'next/link';
import { useState } from 'react';
import { useTranslations } from 'next-intl';
import useSWR from 'swr';
import { UserPlus, UserMinus, Ban, LoaderCircle, MessageCircle } from 'lucide-react';
import { useSession } from '@/components/providers';
import { Button } from '@/components/ui/button';
import { Avatar, Loading, LoadError, Empty } from '@/components/common';
import { api, ApiError, hasPermission } from '@/lib/api';

type Relationship = {
  target_id: number;
  relation_type: 'friend' | 'block';
  mutual: boolean;
  target: { id: number; username: string; avatar_url: string | null; country: string };
};
type Relationships = { friends: Relationship[]; blocks: Relationship[] };
function useRelationships() {
  const { user } = useSession();
  return useSWR<Relationships>(user ? ['relationships', user.id] : null, () =>
    api('/me/relationships'),
  );
}
export function RelationshipActions({
  target,
  targetUsername,
}: {
  target: number;
  targetUsername?: string;
}) {
  const t = useTranslations('social');
  const c = useTranslations('common');
  const { user } = useSession();
  const { data, error: loadError, mutate } = useRelationships();
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState('');
  if (!user || user.id === target) return null;
  const canManage = hasPermission(user.permissions, 'social.manage');
  const canMessage = hasPermission(user.permissions, 'chat.send') && !!targetUsername;
  if (!canManage && !canMessage) return null;
  const friend = data?.friends.some((r) => r.target_id === target);
  const blocked = data?.blocks.some((r) => r.target_id === target);
  async function change(kind: 'friend' | 'block', remove = false) {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await api(`/me/relationships/${target}${remove ? `?relation_type=${kind}` : ''}`, {
        method: remove ? 'DELETE' : 'POST',
        ...(remove ? {} : { body: JSON.stringify({ relation_type: kind }) }),
      });
      setConfirm(false);
      await mutate();
    } catch (e) {
      setError(
        e instanceof ApiError && e.code === 'relationship_blocked'
          ? t('blockedError')
          : c('serviceError'),
      );
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="relationship-actions">
      {canMessage && (
        <Button asChild variant="outline">
          <Link href={`/chat?recipient=${encodeURIComponent(targetUsername!)}`}>
            <MessageCircle />
            {t('message')}
          </Link>
        </Button>
      )}
      {canManage &&
        (loadError ? (
          <Button variant="outline" onClick={() => mutate()}>
            {c('retry')}
          </Button>
        ) : (
          <div className="actions">
            {!blocked && (
              <Button
                variant="outline"
                disabled={!data || busy}
                onClick={() => change('friend', friend)}
              >
                {busy ? <LoaderCircle className="spin" /> : friend ? <UserMinus /> : <UserPlus />}
                {t(friend ? 'removeFriend' : 'addFriend')}
              </Button>
            )}
            <Button
              variant="ghost"
              disabled={!data || busy}
              onClick={() => (blocked ? change('block', true) : setConfirm(true))}
            >
              <Ban />
              {t(blocked ? 'unblock' : 'block')}
            </Button>
          </div>
        ))}
      {confirm && (
        <div className="relationship-confirm" role="group" aria-label={t('block')}>
          <p>{t('blockConfirm')}</p>
          <div className="actions">
            <Button disabled={busy} onClick={() => change('block')}>
              {t('confirmBlock')}
            </Button>
            <Button variant="ghost" disabled={busy} onClick={() => setConfirm(false)}>
              {c('cancel')}
            </Button>
          </div>
        </div>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
export function RelationshipList() {
  const t = useTranslations('social');
  const { data, error, mutate } = useRelationships();
  if (error) return <LoadError retry={() => mutate()} />;
  if (!data) return <Loading />;
  return (
    <div>
      <p className="hint">{t('hint')}</p>
      {(['friends', 'blocks'] as const).map((kind) => (
        <section className="relationship-section" key={kind}>
          <h2>{t(kind)}</h2>
          {!data[kind].length ? (
            <Empty title={t(kind === 'friends' ? 'emptyFriends' : 'emptyBlocks')} />
          ) : (
            <ul>
              {data[kind].map((r) => (
                <li className="relationship-row" key={r.target_id}>
                  <Link className="player-cell" href={`/players/${r.target_id}`}>
                    <Avatar name={r.target.username} url={r.target.avatar_url} />
                    <span>
                      {r.target.username}
                      {r.mutual && kind === 'friends' && <small>{t('mutual')}</small>}
                    </span>
                  </Link>
                  <RelationshipActions target={r.target_id} targetUsername={r.target.username} />
                </li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </div>
  );
}
