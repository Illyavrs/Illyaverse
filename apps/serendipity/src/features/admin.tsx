'use client';
import { useState, type FormEvent } from 'react';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR from 'swr';
import Link from 'next/link';
import { LoaderCircle, ShieldCheck, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import {
  PageHeading,
  Protected,
  Loading,
  LoadError,
  Empty,
  Pagination,
  Avatar,
} from '@/components/common';
import { useSession } from '@/components/providers';
import { api, fetcher, hasPermission } from '@/lib/api';
import type { Page, AdminUser, Role, Beatmap, Score, Audit } from '@/lib/types';

const grants = [
  'chat.send',
  'social.manage',
  'beatmaps.upload',
  'beatmaps.import',
  'beatmaps.moderate',
  'users.moderate',
  'roles.manage',
  'audit.read',
  'scores.moderate',
  'admin.view',
];
type Section = 'users' | 'roles' | 'beatmaps' | 'scores' | 'audit';
function UserRoles({
  player,
  roles,
  changed,
}: {
  player: AdminUser;
  roles: Role[];
  changed: () => void;
}) {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  const [chosen, setChosen] = useState(player.roles.map((r) => r.id));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  return (
    <div className="admin-user-roles">
      <div className="permission-list">
        {roles.map((role) => (
          <label key={role.id}>
            <input
              type="checkbox"
              checked={chosen.includes(role.id)}
              onChange={(e) =>
                setChosen((old) =>
                  e.target.checked ? [...old, role.id] : old.filter((id) => id !== role.id),
                )
              }
            />
            {role.name}
          </label>
        ))}
      </div>
      <Button
        variant="outline"
        size="sm"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setError(false);
          try {
            await api(`/admin/users/${player.id}`, {
              method: 'PATCH',
              body: JSON.stringify({ roles: chosen }),
            });
            changed();
          } catch {
            setError(true);
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? <LoaderCircle className="spin" /> : <ShieldCheck />}
        {t('assignRole')}
      </Button>
      {error && (
        <p className="form-error" role="alert">
          {c('serviceError')}
        </p>
      )}
    </div>
  );
}
function RolesEditor({ changed }: { changed: () => void }) {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  const { data, error, isLoading, mutate } = useSWR<Page<Role>>('/admin/roles', fetcher);
  const [edit, setEdit] = useState<Role | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [failed, setFailed] = useState(false);
  async function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = e.currentTarget;
    const data = new FormData(form);
    setBusy(true);
    setFailed(false);
    setFeedback('');
    try {
      await api(edit ? `/admin/roles/${edit.id}` : '/admin/roles', {
        method: edit ? 'PATCH' : 'POST',
        body: JSON.stringify({
          name: data.get('name'),
          description: data.get('description') ?? '',
          permissions: chosen,
        }),
      });
      await mutate();
      changed();
      setEdit(null);
      setChosen([]);
      form.reset();
      setFeedback(t('saved'));
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      {isLoading ? (
        <Loading />
      ) : error ? (
        <LoadError retry={() => mutate()} />
      ) : (
        <div className="role-list">
          {data?.items.map((role) => (
            <div className="role-row" key={role.id}>
              <div className="section-heading">
                <div>
                  <strong>{role.name}</strong>
                  <p className="hint">{role.description}</p>
                </div>
                {role.name !== 'administrator' && (
                  <Button
                    variant="ghost"
                    onClick={() => {
                      setEdit(role);
                      setChosen(role.permissions);
                    }}
                  >
                    {t('editRole')}
                  </Button>
                )}
              </div>
              <div className="permission-list">
                {role.permissions.map((p) => (
                  <span key={p}>{p}</span>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
      <form className="admin-role-form" onSubmit={submit} key={edit?.id ?? 'create'}>
        <h2>{t(edit ? 'editRole' : 'newRole')}</h2>
        <div className="field">
          <label htmlFor="roleName">{t('roleName')}</label>
          <Input
            id="roleName"
            name="name"
            minLength={3}
            maxLength={32}
            pattern="[A-Za-z0-9_-]+"
            required
            defaultValue={edit?.name ?? ''}
          />
        </div>
        <div className="field">
          <label htmlFor="roleDescription">{t('description')}</label>
          <Textarea
            id="roleDescription"
            name="description"
            maxLength={500}
            defaultValue={edit?.description ?? ''}
          />
        </div>
        <fieldset className="field permission-checkboxes">
          <legend>{t('permissions')}</legend>
          {grants.map((permission) => (
            <label key={permission}>
              <input
                type="checkbox"
                checked={chosen.includes(permission)}
                onChange={(e) =>
                  setChosen((old) =>
                    e.target.checked ? [...old, permission] : old.filter((p) => p !== permission),
                  )
                }
              />
              <span>{permission}</span>
            </label>
          ))}
        </fieldset>
        {failed && (
          <p className="form-error" role="alert">
            {c('serviceError')}
          </p>
        )}
        {feedback && (
          <p className="form-success" role="status">
            {feedback}
          </p>
        )}
        <div className="actions">
          <Button disabled={busy}>
            {busy ? <LoaderCircle className="spin" /> : <Plus />}
            {edit ? c('save') : t('create')}
          </Button>
          {edit && (
            <Button
              variant="ghost"
              type="button"
              onClick={() => {
                setEdit(null);
                setChosen([]);
              }}
            >
              {c('cancel')}
            </Button>
          )}
        </div>
      </form>
    </>
  );
}
function AdminContents() {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  const f = useFormatter();
  const { user } = useSession();
  const allowed = (p: string) => hasPermission(user?.permissions ?? [], p);
  const sections: Section[] = [];
  if (allowed('users.moderate')) sections.push('users');
  if (allowed('roles.manage')) sections.push('roles');
  if (allowed('beatmaps.moderate')) sections.push('beatmaps');
  if (allowed('scores.moderate')) sections.push('scores');
  if (allowed('audit.read')) sections.push('audit');
  const [selected, setSelected] = useState<Section | null>(null);
  const section = selected && sections.includes(selected) ? selected : sections[0];
  const [page, setPage] = useState(1);
  const key =
    section === 'scores'
      ? `/scores?verification=pending&page=${page}&page_size=24`
      : section
        ? `/admin/${section}?page=${page}&page_size=24`
        : null;
  const { data, error, isLoading, mutate } = useSWR<Page<AdminUser | Beatmap | Score | Audit>>(
    section === 'roles' ? null : key,
    fetcher,
  );
  const { data: roles, mutate: refreshRoles } = useSWR<Page<Role>>(
    allowed('roles.manage') ? '/admin/roles' : null,
    fetcher,
  );
  const [busy, setBusy] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);
  const [feedback, setFeedback] = useState(false);
  async function change(path: string, id: number, value: unknown) {
    setBusy(id);
    setFailed(false);
    setFeedback(false);
    try {
      await api(path, { method: 'PATCH', body: JSON.stringify(value) });
      await mutate();
      setFeedback(true);
    } catch {
      setFailed(true);
    } finally {
      setBusy(null);
    }
  }
  return (
    <>
      <div className="admin-toolbar" role="group">
        {sections.map((s) => (
          <button
            key={s}
            aria-pressed={section === s}
            onClick={() => {
              setSelected(s);
              setPage(1);
              setFailed(false);
              setFeedback(false);
            }}
          >
            {t(s)}
          </button>
        ))}
      </div>
      {failed && (
        <p className="form-error" role="alert">
          {c('serviceError')}
        </p>
      )}
      {feedback && (
        <p className="form-success" role="status">
          {t('saved')}
        </p>
      )}
      <section className="glass content-panel">
        {section === 'roles' ? (
          <RolesEditor changed={() => refreshRoles()} />
        ) : !section ? (
          <Empty title={c('noPermission')} />
        ) : isLoading ? (
          <Loading />
        ) : error ? (
          <LoadError retry={() => mutate()} />
        ) : !data?.items.length ? (
          <Empty title={t('empty')} />
        ) : (
          <div className="table-scroll">
            <table className="data-table">
              <thead>
                <tr>
                  {(section === 'users'
                    ? [c('username'), c('status'), t('roles'), t('action')]
                    : section === 'beatmaps'
                      ? [t('beatmaps'), c('status'), t('action')]
                      : section === 'scores'
                        ? [c('username'), c('score'), c('replay'), t('action')]
                        : [c('date'), t('actor'), t('action'), t('target')]
                  ).map((label) => (
                    <th key={label}>{label}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {data.items.map((item) =>
                  section === 'users'
                    ? (() => {
                        const u = item as AdminUser;
                        return (
                          <tr key={u.id}>
                            <td>
                              <Link href={`/players/${u.id}`} className="player-cell">
                                <Avatar name={u.username} url={u.avatar_url} />
                                {u.username}
                              </Link>
                            </td>
                            <td>
                              {u.is_restricted ? (
                                <Badge variant="destructive">{t('restricted')}</Badge>
                              ) : (
                                <Badge variant="secondary">{t('active')}</Badge>
                              )}
                            </td>
                            <td>
                              {roles ? (
                                <UserRoles
                                  key={JSON.stringify(u.roles)}
                                  player={u}
                                  roles={roles.items}
                                  changed={() => mutate()}
                                />
                              ) : (
                                u.roles.map((r) => r.name).join(', ')
                              )}
                            </td>
                            <td>
                              <Button
                                variant="outline"
                                size="sm"
                                disabled={busy === u.id || u.id === user?.id}
                                onClick={() =>
                                  change(`/admin/users/${u.id}`, u.id, {
                                    is_restricted: !u.is_restricted,
                                  })
                                }
                              >
                                {t(u.is_restricted ? 'unrestrict' : 'restrict')}
                              </Button>
                            </td>
                          </tr>
                        );
                      })()
                    : section === 'beatmaps'
                      ? (() => {
                          const b = item as Beatmap;
                          return (
                            <tr key={b.id}>
                              <td>
                                <Link href={`/beatmaps/${b.id}`} className="score-title">
                                  {b.title}
                                  <span>{b.version}</span>
                                </Link>
                              </td>
                              <td>{c(b.status)}</td>
                              <td>
                                <select
                                  className="native-select"
                                  aria-label={t('moderate')}
                                  value={b.status}
                                  disabled={busy === b.id}
                                  onChange={(e) =>
                                    change(`/admin/beatmaps/${b.id}`, b.id, {
                                      status: e.target.value,
                                    })
                                  }
                                >
                                  {['pending', 'ranked', 'loved', 'unranked'].map((s) => (
                                    <option key={s} value={s}>
                                      {c(s)}
                                    </option>
                                  ))}
                                </select>
                              </td>
                            </tr>
                          );
                        })()
                      : section === 'scores'
                        ? (() => {
                            const s = item as Score;
                            return (
                              <tr key={s.id}>
                                <td>
                                  <Link href={`/players/${s.user_id}`}>{s.username}</Link>
                                </td>
                                <td>
                                  {s.title}
                                  <div className="hint">
                                    {f.number(s.score)} · {s.accuracy.toFixed(2)}% ·{' '}
                                    {s.pp.toFixed(2)} pp
                                  </div>
                                </td>
                                <td>
                                  {s.has_replay ? (
                                    <a href={`/api/v1/scores/${s.id}/replay`} className="text-link">
                                      {c('download')}
                                    </a>
                                  ) : (
                                    '—'
                                  )}
                                </td>
                                <td>
                                  <div className="actions">
                                    <Button
                                      size="sm"
                                      disabled={busy === s.id}
                                      onClick={() =>
                                        change(`/admin/scores/${s.id}`, s.id, {
                                          verification: 'verified',
                                        })
                                      }
                                    >
                                      {t('approve')}
                                    </Button>
                                    <Button
                                      size="sm"
                                      variant="outline"
                                      disabled={busy === s.id}
                                      onClick={() =>
                                        change(`/admin/scores/${s.id}`, s.id, {
                                          verification: 'rejected',
                                        })
                                      }
                                    >
                                      {t('reject')}
                                    </Button>
                                  </div>
                                </td>
                              </tr>
                            );
                          })()
                        : (() => {
                            const a = item as Audit;
                            return (
                              <tr key={a.id}>
                                <td>
                                  {f.dateTime(new Date(a.created_at), {
                                    dateStyle: 'short',
                                    timeStyle: 'short',
                                  })}
                                </td>
                                <td>{a.actor_id ?? '—'}</td>
                                <td>{a.action}</td>
                                <td>
                                  {a.target_type} #{a.target_id}
                                </td>
                              </tr>
                            );
                          })(),
                )}
              </tbody>
            </table>
          </div>
        )}
      </section>
      {data && <Pagination page={page} total={data.total} onChange={setPage} />}
    </>
  );
}
export function AdminPage() {
  const t = useTranslations('admin');
  return (
    <div className="page-container">
      <PageHeading title={t('title')} description={t('subtitle')} />
      <Protected permission="admin.view">
        <AdminContents />
      </Protected>
    </div>
  );
}
