'use client';
import { useRef, useState, type FormEvent, type ReactNode } from 'react';
import { useFormatter, useTranslations } from 'next-intl';
import useSWR, { useSWRConfig } from 'swr';
import Link from 'next/link';
import { LoaderCircle, ShieldCheck, Plus, Search } from 'lucide-react';
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
import { api, ApiError, fetcher, hasPermission } from '@/lib/api';
import type { Page, AdminUser, Role, Beatmap, Score, Audit } from '@/lib/types';

const grants = [
  'chat.send',
  'social.manage',
  'beatmaps.upload',
  'beatmaps.import',
  'beatmaps.moderate',
  'users.moderate',
  'users.sessions.revoke',
  'roles.manage',
  'audit.read',
  'scores.moderate',
  'admin.view',
];
type Section = 'users' | 'roles' | 'beatmaps' | 'scores' | 'audit';
type RefreshAdmin = () => Promise<boolean>;
const errorKeys = {
  builtin_role: 'errorBuiltinRole',
  last_administrator: 'errorLastAdministrator',
  stronger_role: 'errorStrongerRole',
  invalid_search: 'errorInvalidSearch',
  role_assigned: 'errorRoleAssigned',
  forbidden: 'errorForbidden',
  unauthorized: 'errorUnauthorized',
} as const;
function useAdminError() {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  return (error: unknown) => {
    if (error instanceof ApiError && error.status >= 400 && error.status < 500) {
      const key = errorKeys[error.code as keyof typeof errorKeys];
      return key ? t(key) : error.message;
    }
    return c('serviceError');
  };
}
function canDelegate(role: Role, permissions: string[]) {
  return role.permissions.every((permission) => hasPermission(permissions, permission));
}
function Feedback({
  error,
  success,
  stale = false,
}: {
  error?: string;
  success?: string;
  stale?: boolean;
}) {
  const t = useTranslations('admin');
  return (
    <>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {success && (
        <p className="form-success" role="status">
          {success}
        </p>
      )}
      {stale && (
        <p className="hint admin-refresh-note" role="status">
          {t('refreshFailed')}
        </p>
      )}
    </>
  );
}
function InlineConfirm({
  id,
  title,
  children,
  confirm,
  busy,
  onConfirm,
  onCancel,
}: {
  id: string;
  title: string;
  children: ReactNode;
  confirm: string;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const c = useTranslations('common');
  return (
    <div
      className="admin-confirm"
      id={id}
      role="group"
      aria-labelledby={`${id}-title`}
      aria-busy={busy}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && !busy) {
          event.preventDefault();
          onCancel();
        }
      }}
    >
      <strong id={`${id}-title`}>{title}</strong>
      <p className="hint">{children}</p>
      <div className="actions">
        <Button
          className="admin-danger-action"
          variant="outline"
          size="sm"
          disabled={busy}
          autoFocus
          onClick={onConfirm}
        >
          {busy && <LoaderCircle className="spin" aria-hidden="true" />}
          {confirm}
        </Button>
        <Button variant="ghost" size="sm" disabled={busy} onClick={onCancel}>
          {c('cancel')}
        </Button>
      </div>
    </div>
  );
}
function UserRoles({
  player,
  roles,
  permissions,
  changed,
}: {
  player: AdminUser;
  roles: Role[];
  permissions: string[];
  changed: RefreshAdmin;
}) {
  const t = useTranslations('admin');
  const describeError = useAdminError();
  const [changes, setChanges] = useState<Record<number, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [stale, setStale] = useState(false);
  const available = [
    ...roles,
    ...player.roles.filter((role) => !roles.some((known) => known.id === role.id)),
  ];
  const editableIds = new Set(
    roles.filter((role) => canDelegate(role, permissions)).map((role) => role.id),
  );
  // Keep server memberships authoritative unless this operator explicitly edits
  // a role they can still delegate. Newly returned or unseen roles are preserved.
  const chosen = Array.from(
    new Set([
      ...player.roles
        .filter((role) => !editableIds.has(role.id) || changes[role.id] !== false)
        .map((role) => role.id),
      ...roles
        .filter((role) => editableIds.has(role.id) && changes[role.id] === true)
        .map((role) => role.id),
    ]),
  );
  const locked = available.some(
    (role) => !('permissions' in role) || !canDelegate(role as Role, permissions),
  );
  const unchanged =
    chosen.length === player.roles.length && player.roles.every((role) => chosen.includes(role.id));
  return (
    <div className="admin-user-roles" aria-busy={busy}>
      <fieldset
        className="permission-list"
        aria-label={t('rolesFor', { name: player.username })}
        disabled={busy || stale}
      >
        {available.map((role) => {
          const enabled = 'permissions' in role && canDelegate(role as Role, permissions);
          return (
            <label
              key={role.id}
              aria-disabled={!enabled}
              title={!enabled ? t('roleLocked') : undefined}
            >
              <input
                type="checkbox"
                checked={chosen.includes(role.id)}
                disabled={!enabled}
                onChange={(event) =>
                  setChanges((old) => ({ ...old, [role.id]: event.target.checked }))
                }
              />
              {role.name}
            </label>
          );
        })}
      </fieldset>
      {locked && <p className="hint admin-access-hint">{t('roleLocked')}</p>}
      <Button
        variant="outline"
        size="sm"
        disabled={busy || stale || unchanged}
        onClick={async () => {
          if (busy || stale || unchanged) return;
          setBusy(true);
          setError('');
          setSuccess('');
          try {
            await api(`/admin/users/${player.id}`, {
              method: 'PATCH',
              body: JSON.stringify({ roles: chosen }),
            });
            setSuccess(t('saved'));
            const refreshed = await changed();
            setStale(!refreshed);
            if (refreshed) setChanges({});
          } catch (error) {
            setError(describeError(error));
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? (
          <LoaderCircle className="spin" aria-hidden="true" />
        ) : (
          <ShieldCheck aria-hidden="true" />
        )}
        {t('assignRole')}
      </Button>
      <Feedback error={error} success={success} stale={stale} />
    </div>
  );
}
function UserActions({
  player,
  permissions,
  changed,
}: {
  player: AdminUser;
  permissions: string[];
  changed: RefreshAdmin;
}) {
  const t = useTranslations('admin');
  const describeError = useAdminError();
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [stale, setStale] = useState(false);
  const revokeButton = useRef<HTMLButtonElement>(null);
  const moderate = hasPermission(permissions, 'users.moderate') && player.can_moderate;
  const revoke = hasPermission(permissions, 'users.sessions.revoke') && player.can_revoke_sessions;
  function cancel() {
    setConfirm(false);
    queueMicrotask(() => revokeButton.current?.focus());
  }
  async function write(revoking = false) {
    if (busy || stale) return;
    setBusy(true);
    setError('');
    setSuccess('');
    try {
      if (revoking) {
        const result = await api<{ revoked: { web: number; stable: number; lazer: number } }>(
          `/admin/users/${player.id}/sessions/revoke`,
          { method: 'POST' },
        );
        setSuccess(t('sessionsRevoked', result.revoked));
        setConfirm(false);
      } else {
        await api(`/admin/users/${player.id}`, {
          method: 'PATCH',
          body: JSON.stringify({ is_restricted: !player.is_restricted }),
        });
        setSuccess(t('saved'));
      }
      setStale(!(await changed()));
    } catch (error) {
      setError(describeError(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="admin-user-actions" aria-busy={busy}>
      <div className="actions">
        {moderate && (
          <Button variant="outline" size="sm" disabled={busy || stale} onClick={() => write()}>
            {busy && !confirm && <LoaderCircle className="spin" aria-hidden="true" />}
            {t(player.is_restricted ? 'unrestrict' : 'restrict')}
          </Button>
        )}
        {revoke && (
          <Button
            ref={revokeButton}
            variant="outline"
            size="sm"
            disabled={busy || stale}
            aria-expanded={confirm}
            aria-controls={`revoke-${player.id}`}
            onClick={() => {
              setConfirm(true);
              setError('');
              setSuccess('');
            }}
          >
            {t('revokeSessions')}
          </Button>
        )}
      </div>
      {!moderate && !revoke && <p className="hint admin-access-hint">{t('actionsReadOnly')}</p>}
      {confirm && (
        <InlineConfirm
          id={`revoke-${player.id}`}
          title={t('revokeConfirm', { name: player.username })}
          confirm={t('confirmRevoke')}
          busy={busy}
          onConfirm={() => write(true)}
          onCancel={cancel}
        >
          {t('revokeHint')}
        </InlineConfirm>
      )}
      <Feedback error={error} success={success} stale={stale} />
    </div>
  );
}
function RolesEditor({ changed, permissions }: { changed: RefreshAdmin; permissions: string[] }) {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  const describeError = useAdminError();
  const { data, error: loadError, isLoading, mutate } = useSWR<Page<Role>>('/admin/roles', fetcher);
  const [edit, setEdit] = useState<Role | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [deleting, setDeleting] = useState<Role | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [deleteError, setDeleteError] = useState('');
  const [success, setSuccess] = useState('');
  const [stale, setStale] = useState(false);
  const formRef = useRef<HTMLFormElement>(null);
  const deleteButton = useRef<HTMLButtonElement | null>(null);
  const permissionChoices = Array.from(new Set([...grants, ...chosen]));
  const editable = !edit || (canDelegate(edit, permissions) && !edit.permissions.includes('*'));
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || stale || !editable) return;
    const form = event.currentTarget;
    const values = new FormData(form);
    setBusy(true);
    setError('');
    setSuccess('');
    try {
      await api(edit ? `/admin/roles/${edit.id}` : '/admin/roles', {
        method: edit ? 'PATCH' : 'POST',
        body: JSON.stringify({
          name: values.get('name'),
          description: values.get('description') ?? '',
          permissions: chosen,
        }),
      });
      setEdit(null);
      setChosen([]);
      form.reset();
      setSuccess(t('saved'));
      setStale(!(await changed()));
    } catch (error) {
      setError(describeError(error));
    } finally {
      setBusy(false);
    }
  }
  async function remove() {
    if (!deleting || busy || stale) return;
    setBusy(true);
    setDeleteError('');
    setSuccess('');
    try {
      await api(`/admin/roles/${deleting.id}`, { method: 'DELETE' });
      if (edit?.id === deleting.id) {
        setEdit(null);
        setChosen([]);
      }
      setDeleting(null);
      setSuccess(t('roleDeleted'));
      setStale(!(await changed()));
    } catch (error) {
      setDeleteError(describeError(error));
    } finally {
      setBusy(false);
    }
  }
  function cancelDelete() {
    setDeleting(null);
    setDeleteError('');
    queueMicrotask(() => deleteButton.current?.focus());
  }
  return (
    <div aria-busy={busy}>
      {isLoading ? (
        <Loading />
      ) : loadError && !data ? (
        <LoadError retry={() => mutate()} />
      ) : (
        <div className="role-list">
          {data?.items.map((role) => {
            const delegable = canDelegate(role, permissions);
            const wildcard = role.permissions.includes('*');
            const editAllowed = delegable && !wildcard;
            const deleteAllowed = editAllowed && !role.is_builtin && role.membership_count === 0;
            const lockReason = wildcard
              ? t('operatorRoleReadOnly')
              : !delegable
                ? t('roleLocked')
                : '';
            const deleteReason =
              lockReason ||
              (role.is_builtin
                ? t('builtinRoleHint')
                : role.membership_count > 0
                  ? t('assignedRoleHint')
                  : '');
            return (
              <div className="role-row" key={role.id} data-role-id={role.id}>
                <div className="section-heading">
                  <div>
                    <strong>{role.name}</strong>
                    <p className="hint">{role.description}</p>
                    <p className="hint admin-role-meta">
                      {t('memberships', { count: role.membership_count })}
                      {role.is_builtin && ` · ${t('builtinRole')}`}
                    </p>
                  </div>
                  <div className="actions">
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={!editAllowed || busy || stale}
                      title={lockReason || undefined}
                      onClick={() => {
                        setEdit(role);
                        setChosen(role.permissions);
                        setError('');
                        setSuccess('');
                        setDeleting(null);
                        queueMicrotask(() =>
                          formRef.current?.querySelector<HTMLTextAreaElement>('textarea')?.focus(),
                        );
                      }}
                    >
                      {t('editRole')}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={!deleteAllowed || busy || stale}
                      title={deleteReason || undefined}
                      aria-expanded={deleting?.id === role.id}
                      aria-controls={`delete-${role.id}`}
                      onClick={(event) => {
                        deleteButton.current = event.currentTarget;
                        setDeleting(role);
                        setDeleteError('');
                        setSuccess('');
                      }}
                    >
                      {t('deleteRole')}
                    </Button>
                  </div>
                </div>
                <div className="permission-list">
                  {role.permissions.map((permission) => (
                    <span key={permission}>{permission}</span>
                  ))}
                </div>
                {(lockReason || deleteReason) && (
                  <p className="hint admin-access-hint">{lockReason || deleteReason}</p>
                )}
                {deleting?.id === role.id && (
                  <>
                    <InlineConfirm
                      id={`delete-${role.id}`}
                      title={t('deleteConfirm', { name: role.name })}
                      confirm={t('confirmDelete')}
                      busy={busy}
                      onConfirm={remove}
                      onCancel={cancelDelete}
                    >
                      {t('deleteHint')}
                    </InlineConfirm>
                    <Feedback error={deleteError} />
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
      <Feedback success={success} stale={stale} />
      <form ref={formRef} className="admin-role-form" onSubmit={submit} key={edit?.id ?? 'create'}>
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
            readOnly={edit?.is_builtin}
            disabled={busy || stale || !editable}
            aria-describedby={edit?.is_builtin ? 'builtin-role-name-hint' : 'role-name-hint'}
          />
          <p className="hint" id={edit?.is_builtin ? 'builtin-role-name-hint' : 'role-name-hint'}>
            {t(edit?.is_builtin ? 'builtinRoleHint' : 'roleNameHint')}
          </p>
        </div>
        <div className="field">
          <label htmlFor="roleDescription">{t('description')}</label>
          <Textarea
            id="roleDescription"
            name="description"
            maxLength={500}
            defaultValue={edit?.description ?? ''}
            disabled={busy || stale || !editable}
          />
        </div>
        <fieldset
          className="field permission-checkboxes"
          disabled={busy || stale || !editable}
          aria-describedby="role-permission-hint"
        >
          <legend>{t('permissions')}</legend>
          {permissionChoices.map((permission) => (
            <label key={permission} aria-disabled={!hasPermission(permissions, permission)}>
              <input
                type="checkbox"
                checked={chosen.includes(permission)}
                disabled={!hasPermission(permissions, permission)}
                onChange={(event) =>
                  setChosen((old) =>
                    event.target.checked
                      ? [...old, permission]
                      : old.filter((value) => value !== permission),
                  )
                }
              />
              <span>{permission}</span>
            </label>
          ))}
        </fieldset>
        <p className="hint admin-access-hint" id="role-permission-hint">
          {t('roleHint')}
        </p>
        <Feedback error={error} />
        <div className="actions">
          <Button disabled={busy || stale || !editable}>
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" />
            ) : (
              <Plus aria-hidden="true" />
            )}
            {edit ? c('save') : t('create')}
          </Button>
          {edit && (
            <Button
              variant="ghost"
              type="button"
              disabled={busy}
              onClick={() => {
                setEdit(null);
                setChosen([]);
                setError('');
              }}
            >
              {c('cancel')}
            </Button>
          )}
        </div>
      </form>
    </div>
  );
}

function AdminContents() {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  const f = useFormatter();
  const { user, refresh } = useSession();
  const { mutate: refreshCache, cache } = useSWRConfig();
  const describeError = useAdminError();
  const permissions = user?.permissions ?? [];
  const allowed = (p: string) => hasPermission(user?.permissions ?? [], p);
  const sections: Section[] = [];
  if (['users.moderate', 'roles.manage', 'users.sessions.revoke'].some(allowed))
    sections.push('users');
  if (allowed('roles.manage')) sections.push('roles');
  if (allowed('beatmaps.moderate')) sections.push('beatmaps');
  if (allowed('scores.moderate')) sections.push('scores');
  if (allowed('audit.read')) sections.push('audit');
  const [selected, setSelected] = useState<Section | null>(null);
  const section = selected && sections.includes(selected) ? selected : sections[0];
  const [page, setPage] = useState(1);
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const key =
    section === 'scores'
      ? `/scores?verification=pending&page=${page}&page_size=24`
      : section
        ? `/admin/${section}?page=${page}&page_size=24${section === 'users' && search ? `&search=${encodeURIComponent(search)}` : ''}`
        : null;
  const { data, error, isLoading, mutate } = useSWR<Page<AdminUser | Beatmap | Score | Audit>>(
    section === 'roles' ? null : key,
    fetcher,
    {
      keepPreviousData: false,
      // A decision can return to a page whose cached response predates the write.
      dedupingInterval: 0,
      onSuccess: (result, requestedKey) => {
        // Only a response for this collection and page can shrink its pagination.
        if (requestedKey !== key || result.page !== page || result.page_size < 1) return;
        const lastPage = Math.max(1, Math.ceil(result.total / result.page_size));
        setPage((current) => current === result.page && current > lastPage ? lastPage : current);
      },
    },
  );
  const {
    data: roles,
    error: rolesError,
    mutate: refreshRoles,
  } = useSWR<Page<Role>>(allowed('roles.manage') ? '/admin/roles' : null, fetcher);
  const [busy, setBusy] = useState<number | null>(null);
  const [failed, setFailed] = useState('');
  const [feedback, setFeedback] = useState(false);
  const [stale, setStale] = useState(false);
  async function refreshAdministration() {
    const adminKey = (key: unknown): key is string =>
      typeof key === 'string' &&
      (key.startsWith('/admin/users') ||
        key.startsWith('/admin/roles') ||
        key.startsWith('/me/sessions'));
    const keys = Array.from(cache.keys()).filter(adminKey);
    const results = await Promise.allSettled([refreshCache(adminKey), refresh()]);
    // SWR revalidation can resolve with cached data after a failed fetch.
    return (
      results.every((result) => result.status === 'fulfilled') &&
      keys.every((key) => !cache.get(key)?.error)
    );
  }
  async function change(path: string, id: number, value: unknown) {
    if (busy !== null || stale) return;
    setBusy(id);
    setFailed('');
    setFeedback(false);
    try {
      await api(path, { method: 'PATCH', body: JSON.stringify(value) });
      setFeedback(true);
      const results = await Promise.allSettled([mutate(), refreshAdministration()]);
      setStale(results.some((result) => result.status === 'rejected' || result.value === false));
    } catch (error) {
      setFailed(describeError(error));
    } finally {
      setBusy(null);
    }
  }
  return (
    <>
      <div className="admin-toolbar" role="group" aria-label={t('sections')}>
        {sections.map((s) => (
          <button
            key={s}
            aria-pressed={section === s}
            onClick={() => {
              setSelected(s);
              setPage(1);
              setFailed('');
              setFeedback(false);
            }}
          >
            {t(s)}
          </button>
        ))}
      </div>
      <Feedback error={failed} success={feedback ? t('saved') : ''} stale={stale} />
      {section === 'users' && (
        <form
          className="admin-search"
          onSubmit={(event) => {
            event.preventDefault();
            setSearch(searchInput.trim());
            setPage(1);
            setFailed('');
          }}
        >
          <div className="field">
            <label htmlFor="admin-user-search">{t('searchUsers')}</label>
            <Input
              id="admin-user-search"
              type="search"
              maxLength={64}
              value={searchInput}
              onChange={(event) => setSearchInput(event.target.value)}
              aria-describedby="admin-search-hint"
            />
            <p className="hint" id="admin-search-hint">
              {t('searchHint')}
            </p>
          </div>
          <div className="actions">
            <Button type="submit" variant="outline">
              <Search aria-hidden="true" />
              {c('search')}
            </Button>
            {search && (
              <Button
                type="button"
                variant="ghost"
                onClick={() => {
                  setSearch('');
                  setSearchInput('');
                  setPage(1);
                }}
              >
                {t('clearSearch')}
              </Button>
            )}
          </div>
        </form>
      )}
      <section className="glass content-panel">
        {section === 'users' && (
          <p className="hint admin-scroll-hint" id="admin-scroll-hint">
            {t('scrollUsers')}
          </p>
        )}
        {section === 'roles' ? (
          <RolesEditor changed={refreshAdministration} permissions={permissions} />
        ) : !section ? (
          <Empty title={c('noPermission')} />
        ) : isLoading ? (
          <Loading />
        ) : error && !data ? (
          <>
            <Feedback error={describeError(error)} />
            <LoadError retry={() => mutate()} />
          </>
        ) : !data?.items.length ? (
          <Empty title={t(search && section === 'users' ? 'noSearchResults' : 'empty')} />
        ) : (
          <div
            className={`table-scroll${section === 'users' ? ' admin-users-scroll' : ''}`}
            role="region"
            aria-label={t(section)}
            aria-describedby={section === 'users' ? 'admin-scroll-hint' : undefined}
            tabIndex={0}
          >
            <table className={`data-table${section === 'users' ? ' admin-users-table' : ''}`}>
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
                          <tr key={u.id} className="admin-user-row" data-user-id={u.id}>
                            <td>
                              <Link href={`/players/${u.id}`} className="player-cell">
                                <Avatar name={u.username} url={u.avatar_url} />
                                {u.username}
                              </Link>
                              <p className="hint admin-player-id">
                                #{u.id} · {u.country}
                              </p>
                            </td>
                            <td>
                              {u.is_restricted ? (
                                <Badge variant="destructive">{t('restricted')}</Badge>
                              ) : (
                                <Badge variant="secondary">{t('active')}</Badge>
                              )}
                            </td>
                            <td>
                              {allowed('roles.manage') && u.can_manage_roles && roles ? (
                                <UserRoles
                                  player={u}
                                  roles={roles.items}
                                  permissions={permissions}
                                  changed={refreshAdministration}
                                />
                              ) : (
                                <div className="admin-user-roles">
                                  <div className="permission-list">
                                    {u.roles.map((role) => (
                                      <span key={role.id}>{role.name}</span>
                                    ))}
                                  </div>
                                  {allowed('roles.manage') && (
                                    <p className="hint admin-access-hint">
                                      {t(u.can_manage_roles ? 'rolesLoading' : 'rolesReadOnly')}
                                    </p>
                                  )}
                                  {u.can_manage_roles && rolesError && allowed('roles.manage') && (
                                    <>
                                      <Feedback error={describeError(rolesError)} />
                                      <Button
                                        variant="ghost"
                                        size="sm"
                                        onClick={() => refreshRoles()}
                                      >
                                        {c('retry')}
                                      </Button>
                                    </>
                                  )}
                                </div>
                              )}
                            </td>
                            <td>
                              <UserActions
                                player={u}
                                permissions={permissions}
                                changed={refreshAdministration}
                              />
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
      {section !== 'roles' && data && (
        <Pagination page={page} total={data.total} pageSize={data.page_size} onChange={setPage} />
      )}
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
