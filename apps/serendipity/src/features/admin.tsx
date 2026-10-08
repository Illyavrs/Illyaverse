'use client';
import {
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from 'react';
import { useFormatter, useTranslations } from 'next-intl';
import { useSearchParams } from 'next/navigation';
import useSWR, { unstable_serialize, useSWRConfig } from 'swr';
import { Dialog } from 'radix-ui';
import Link from 'next/link';
import { LoaderCircle, ShieldCheck, Plus, Search } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { FieldSelect } from '@/components/ui/field-select';
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
import type {
  Page,
  AdminUser,
  Role,
  Beatmap,
  BeatmapAvailability,
  Score,
  Audit,
} from '@/lib/types';
import { AdminAnnouncements, type AnnouncementNavigationGuard } from './announcements';

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
  'announcements.manage',
];
type Section = 'users' | 'roles' | 'beatmaps' | 'scores' | 'audit' | 'announcements';
const mapStatuses = ['pending', 'ranked', 'loved', 'unranked'] as const;
const mapModes = ['osu!', 'taiko', 'catch', 'mania'] as const;
type MapStatus = (typeof mapStatuses)[number];
type MapFilters = {
  q: string;
  mode: string;
  source: string;
  status: string;
  availability: string;
  page: number;
};
function readMapFilters(query: Pick<URLSearchParams, 'get'>): MapFilters {
  const mode = query.get('mode') ?? '';
  const source = query.get('source') ?? '';
  const status = query.get('status') ?? '';
  const rawPage = query.get('page') ?? '1';
  const page = /^\d+$/.test(rawPage) ? Number(rawPage) : 1;
  return {
    q: (query.get('q') ?? '').trim(),
    mode: /^[0-3]$/.test(mode) ? mode : 'all',
    source: ['local', 'official'].includes(source) ? source : 'all',
    status: mapStatuses.includes(status as MapStatus) ? status : 'all',
    availability: ['withdrawn', 'all'].includes(query.get('availability') ?? '')
      ? query.get('availability')!
      : 'active',
    page: Number.isSafeInteger(page) && page > 0 && page <= 4294967295 ? page : 1,
  };
}
function mapQuery(query: string, filters: MapFilters) {
  const next = new URLSearchParams(query);
  next.set('tab', 'beatmaps');
  for (const name of ['q', 'mode', 'source', 'status'] as const) {
    const value = filters[name];
    if (value && value !== 'all') next.set(name, value);
    else next.delete(name);
  }
  if (filters.availability !== 'active') next.set('availability', filters.availability);
  else next.delete('availability');
  if (filters.page > 1) next.set('page', String(filters.page));
  else next.delete('page');
  return next;
}
function mapListKey(filters: MapFilters) {
  const query = new URLSearchParams({ page: String(filters.page), page_size: '24' });
  query.set('availability', filters.availability);
  for (const name of ['q', 'mode', 'source', 'status'] as const) {
    if (filters[name] && filters[name] !== 'all') query.set(name, filters[name]);
  }
  return `/admin/beatmaps?${query}`;
}
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

export function BeatmapAvailabilityAction({
  map,
  disabled = false,
  changed,
  onBusy,
  accepted,
}: {
  map: Beatmap;
  disabled?: boolean;
  changed: RefreshAdmin;
  onBusy?: (busy: boolean) => void;
  accepted?: (availability: BeatmapAvailability) => void;
}) {
  const t = useTranslations('mapAvailability');
  const c = useTranslations('common');
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState(false);
  const [reason, setReason] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [saved, setSaved] = useState(false);
  const [stale, setStale] = useState(false);
  const writing = useRef(false);
  const withdrawn = map.availability?.withdrawn ?? false;
  const revision = map.availability?.revision ?? 0;
  const nativeSetId = map.native_set_id ?? map.package?.native_set_id;
  async function refresh() {
    if (writing.current) return;
    writing.current = true;
    setBusy(true);
    try {
      const refreshed = await changed();
      setStale(!refreshed);
      if (refreshed) {
        setConflict(false);
        setError('');
        if (saved) setOpen(false);
      }
    } finally {
      writing.current = false;
      setBusy(false);
    }
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (
      writing.current ||
      disabled ||
      saved ||
      stale ||
      conflict ||
      !nativeSetId ||
      target === withdrawn
    )
      return;
    const trimmed = reason.trim();
    if (
      !trimmed ||
      Array.from(trimmed).length > 500 ||
      new TextEncoder().encode(trimmed).length > 2000
    ) {
      setError(t('reasonInvalid'));
      return;
    }
    writing.current = true;
    setBusy(true);
    onBusy?.(true);
    setError('');
    try {
      const result = await api<{ native_set_id: number; availability: BeatmapAvailability }>(
        `/admin/beatmapsets/${nativeSetId}/availability`,
        {
          method: 'PATCH',
          body: JSON.stringify({ withdrawn: target, expected_revision: revision, reason: trimmed }),
        },
      );
      accepted?.(result.availability);
      // A successful write stays acknowledged even when the following read fails.
      setSaved(true);
      const refreshed = await changed();
      setStale(!refreshed);
      if (refreshed) setOpen(false);
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === 'availability_conflict') {
        setConflict(true);
        setError(t('conflict'));
      } else if (failure instanceof ApiError && [401, 403].includes(failure.status)) {
        setError(t('permissionError'));
      } else {
        setError(t('writeError'));
      }
    } finally {
      writing.current = false;
      setBusy(false);
      onBusy?.(false);
    }
  }
  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!busy) setOpen(next);
      }}
    >
      <Dialog.Trigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={disabled || !nativeSetId || stale}
          onClick={() => {
            setTarget(!withdrawn);
            setReason('');
            setError('');
            setConflict(false);
            setSaved(false);
          }}
        >
          {t(withdrawn ? 'restore' : 'withdraw')}
        </Button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="map-availability-overlay" />
        <Dialog.Content className="map-availability-dialog glass" data-material="floating" aria-busy={busy}>
          <Dialog.Title asChild>
            <h2>{t(target ? 'withdrawTitle' : 'restoreTitle')}</h2>
          </Dialog.Title>
          <Dialog.Description className="hint">
            {t(target ? 'withdrawHint' : 'restoreHint')}
          </Dialog.Description>
          <p className="map-availability-context">
            <strong>
              {map.artist} — {map.title}
            </strong>
            <span>
              {t('setId', { id: nativeSetId ?? 0 })} · {t('revision', { revision })}
            </span>
          </p>
          <form onSubmit={submit}>
            <div className="field">
              <label htmlFor={`set-${map.id}-reason`}>{t('reason')}</label>
              <Textarea
                id={`set-${map.id}-reason`}
                value={reason}
                required
                maxLength={1000}
                disabled={busy || saved}
                aria-describedby={`set-${map.id}-reason-hint`}
                onChange={(event) => setReason(event.target.value)}
              />
              <p className="hint" id={`set-${map.id}-reason-hint`}>
                {t('reasonHint')}
              </p>
            </div>
            {error && (
              <p className="form-error" role="alert">
                {error}
              </p>
            )}
            {saved && (
              <p className="form-success" role="status">
                {t(target ? 'withdrawnSaved' : 'restoredSaved')}
              </p>
            )}
            {stale && (
              <p className="hint" role="status">
                {t('refreshFailed')}
              </p>
            )}
            {!saved && !conflict && target === withdrawn && (
              <p className="hint" role="status">
                {t('alreadyChanged')}
              </p>
            )}
            <div className="actions">
              {stale || conflict ? (
                <Button type="button" variant="outline" disabled={busy} onClick={refresh}>
                  {busy && <LoaderCircle className="spin" aria-hidden="true" />}
                  {t('refresh')}
                </Button>
              ) : (
                <Button
                  type="submit"
                  variant="outline"
                  className={target ? 'admin-danger-action' : undefined}
                  disabled={busy || disabled || saved || target === withdrawn || !reason.trim()}
                >
                  {busy && <LoaderCircle className="spin" aria-hidden="true" />}
                  {t(target ? 'confirmWithdraw' : 'confirmRestore')}
                </Button>
              )}
              <Dialog.Close asChild>
                <Button type="button" variant="ghost" disabled={busy}>
                  {c(saved ? 'close' : 'cancel')}
                </Button>
              </Dialog.Close>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function MapModerationRow({
  map,
  returnTo,
  busy,
  disabled,
  save,
  canModerate,
  changed,
  onBusy,
}: {
  map: Beatmap;
  returnTo: string;
  busy: boolean;
  disabled: boolean;
  save: (map: Beatmap, status: MapStatus) => Promise<boolean>;
  canModerate: boolean;
  changed: RefreshAdmin;
  onBusy: (busy: boolean) => void;
}) {
  const t = useTranslations('mapModeration');
  const c = useTranslations('common');
  const availability = useTranslations('mapAvailability');
  const a = useTranslations('admin');
  const describeError = useAdminError();
  const [chosen, setChosen] = useState<MapStatus | null>(null);
  const [error, setError] = useState('');
  const status = chosen ?? map.status;
  const detail = new URLSearchParams({
    management: '1',
    mode: String(map.mode),
    variant: 'vanilla',
    returnTo,
  });
  return (
    <tr data-beatmap-id={map.id}>
      <td>
        <Link href={`/beatmaps/${map.id}?${detail}`} className="score-title">
          {map.artist} — {map.title}
          <span>{map.version}</span>
        </Link>
        <p className="hint admin-map-meta">
          {map.creator} · #{map.id}
        </p>
      </td>
      <td>{mapModes[map.mode] ?? '—'}</td>
      <td>{c(map.source === 'local' ? 'local' : 'official')}</td>
      <td>
        <span className={`status-text status-${map.status}`}>{c(map.status)}</span>
      </td>
      <td>
        <Badge variant="outline">
          {availability(map.availability?.withdrawn ? 'withdrawn' : 'active')}
        </Badge>
      </td>
      <td>
        {canModerate ? (
          <>
            <form
              className="admin-map-actions"
              aria-label={t('statusFor', { title: map.title })}
              onSubmit={async (event) => {
                event.preventDefault();
                if (disabled || status === map.status) return;
                setError('');
                try {
                  if (await save(map, status as MapStatus)) setChosen(null);
                } catch (error) {
                  setError(describeError(error));
                }
              }}
            >
              <div className="actions">
                <FieldSelect
                  className="native-select"
                  aria-label={t('statusFor', { title: map.title })}
                  aria-describedby={error ? `map-status-${map.id}-error` : undefined}
                  value={status}
                  disabled={disabled}
                  onValueChange={(value) => setChosen(value as MapStatus)}
                  options={mapStatuses.map((value) => ({ value, label: c(value) }))}
                />
                <Button
                  type="submit"
                  size="sm"
                  variant="outline"
                  disabled={disabled || status === map.status}
                >
                  {busy && <LoaderCircle className="spin" aria-hidden="true" />}
                  {t('updateStatus')}
                </Button>
              </div>
              {error && (
                <div id={`map-status-${map.id}-error`}>
                  <Feedback error={error} />
                </div>
              )}
            </form>
            <div className="admin-map-availability-action">
              <BeatmapAvailabilityAction
                map={map}
                disabled={disabled}
                changed={changed}
                onBusy={onBusy}
              />
            </div>
          </>
        ) : (
          <p className="hint">{a('actionsReadOnly')}</p>
        )}
      </td>
    </tr>
  );
}

function MapModeration() {
  const t = useTranslations('mapModeration');
  const a = useTranslations('admin');
  const c = useTranslations('common');
  const availability = useTranslations('mapAvailability');
  const { user, loading } = useSession();
  const canModerate = !!user && hasPermission(user.permissions, 'beatmaps.moderate');
  const query = useSearchParams();
  const filters = readMapFilters(query);
  const key = mapListKey(filters);
  const { cache, mutate: mutateCache } = useSWRConfig();
  const requestKey =
    !loading && user && hasPermission(user.permissions, 'admin.view')
      ? ([key, user.id] as const)
      : null;
  const serializedKey = requestKey ? unstable_serialize(requestKey) : '';
  const [accessFailures, setAccessFailures] = useState<Record<string, ApiError>>({});
  const describeError = useAdminError();
  const [searchError, setSearchError] = useState('');
  const [busy, setBusy] = useState<number | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [stale, setStale] = useState(false);
  const [feedback, setFeedback] = useState('');
  const mounted = useRef(true);
  const saving = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  function currentRequest(requestedKey: string) {
    if (!mounted.current) return false;
    const current = new URLSearchParams(window.location.search);
    const tab = current.get('tab');
    return (!tab || tab === 'beatmaps') && mapListKey(readMapFilters(current)) === requestedKey;
  }
  function select(next: Partial<MapFilters>) {
    const current = new URLSearchParams(window.location.search);
    const params = mapQuery(current.toString(), { ...readMapFilters(current), page: 1, ...next });
    window.history.pushState(null, '', `?${params}`);
    setSearchError('');
  }
  const {
    data: result,
    error: requestError,
    isLoading,
    mutate,
  } = useSWR<Page<Beatmap>>(
    requestKey,
    (key: readonly [string, number]) => fetcher<Page<Beatmap>>(key[0]),
    {
      keepPreviousData: false,
      dedupingInterval: 0,
      shouldRetryOnError: false,
      onError: (failure, requestedKey) => {
        if (!(failure instanceof ApiError) || ![401, 403, 404].includes(failure.status)) return;
        setAccessFailures((current) => ({ ...current, [requestedKey]: failure }));
        void mutateCache(requestedKey, undefined, { revalidate: false });
      },
      onSuccess: (result, requestedKey) => {
        setAccessFailures((current) => {
          if (!current[requestedKey]) return current;
          const next = { ...current };
          delete next[requestedKey];
          return next;
        });
        // A response for a previous filter, page or tab cannot move the current view.
        if (requestedKey !== serializedKey || !currentRequest(key) || result.page_size < 1) return;
        setStale(false);
        const lastPage = Math.max(1, Math.ceil(result.total / result.page_size));
        if (result.page > lastPage) {
          const current = new URLSearchParams(window.location.search);
          const params = mapQuery(current.toString(), {
            ...readMapFilters(current),
            page: lastPage,
          });
          window.history.replaceState(null, '', `?${params}`);
        }
      },
    },
  );
  const error = requestError ?? accessFailures[serializedKey];
  const data =
    loading || !requestKey || (error instanceof ApiError && [401, 403, 404].includes(error.status))
      ? undefined
      : result;
  async function refreshList() {
    setRefreshing(true);
    try {
      const result = await mutate();
      // A failed SWR revalidation may resolve with previously cached records.
      const refreshed = Boolean(result) && !cache.get(serializedKey)?.error;
      if (currentRequest(key)) setStale(!refreshed);
      return refreshed;
    } catch {
      if (currentRequest(key)) setStale(true);
      return false;
    } finally {
      if (mounted.current) setRefreshing(false);
    }
  }
  async function save(map: Beatmap, status: MapStatus) {
    if (saving.current || stale) return false;
    saving.current = true;
    setBusy(map.id);
    setFeedback('');
    try {
      await api(`/admin/beatmaps/${map.id}`, { method: 'PATCH', body: JSON.stringify({ status }) });
      if (mounted.current) setFeedback(t('statusSaved', { title: map.title, status: c(status) }));
      return await refreshList();
    } finally {
      saving.current = false;
      if (mounted.current) setBusy(null);
    }
  }
  const filtered =
    filters.q !== '' ||
    filters.availability !== 'active' ||
    [filters.mode, filters.source, filters.status].some((value) => value !== 'all');
  const returnTo = `/admin?${mapQuery(query.toString(), filters)}`;
  return (
    <div className="admin-map-workbench">
      <div className="section-heading">
        <div>
          <h2>{t('title')}</h2>
          <p className="hint">{t('description')}</p>
        </div>
        <Button
          type="button"
          variant="outline"
          disabled={busy !== null}
          onClick={() => select({ source: 'local', status: 'pending', availability: 'active' })}
        >
          {t('pendingCommunity')}
        </Button>
      </div>
      <form
        className="map-review-filters admin-map-controls"
        aria-label={t('search')}
        onSubmit={(event) => {
          event.preventDefault();
          const values = new FormData(event.currentTarget);
          const search = String(values.get('q') ?? '').trim();
          if (new TextEncoder().encode(search).length > 100) {
            setSearchError(t('searchTooLong'));
            return;
          }
          select({ q: search });
        }}
      >
        <div className="admin-map-search">
          <div className="field">
            <label htmlFor="admin-map-search">{t('search')}</label>
            <Input
              key={filters.q}
              id="admin-map-search"
              name="q"
              type="search"
              maxLength={100}
              defaultValue={filters.q}
              aria-describedby="admin-map-search-hint"
              aria-invalid={Boolean(searchError)}
            />
            <p className="hint" id="admin-map-search-hint">
              {t('searchHint')}
            </p>
          </div>
          <Button type="submit" variant="outline" disabled={busy !== null}>
            <Search aria-hidden="true" />
            {c('search')}
          </Button>
        </div>
        <div className="admin-map-filters admin-map-availability-filters">
          <div className="field">
            <label htmlFor="admin-map-mode">{c('mode')}</label>
            <FieldSelect
              id="admin-map-mode"
              name="mode"
              className="native-select"
              value={filters.mode}
              disabled={busy !== null}
              onValueChange={(value) => select({ mode: value })}
              options={[
                { value: 'all', label: c('all') },
                ...mapModes.map((label, mode) => ({ value: String(mode), label })),
              ]}
            />
          </div>
          <div className="field">
            <label htmlFor="admin-map-source">{t('source')}</label>
            <FieldSelect
              id="admin-map-source"
              name="source"
              className="native-select"
              value={filters.source}
              disabled={busy !== null}
              onValueChange={(value) => select({ source: value })}
              options={[
                { value: 'all', label: c('all') },
                { value: 'local', label: c('local') },
                { value: 'official', label: c('official') },
              ]}
            />
          </div>
          <div className="field">
            <label htmlFor="admin-map-status">{c('status')}</label>
            <FieldSelect
              id="admin-map-status"
              name="status"
              className="native-select"
              value={filters.status}
              disabled={busy !== null}
              onValueChange={(value) => select({ status: value })}
              options={[
                { value: 'all', label: c('all') },
                ...mapStatuses.map((value) => ({ value, label: c(value) })),
              ]}
            />
          </div>
          <div className="field">
            <label htmlFor="admin-map-availability">{availability('label')}</label>
            <FieldSelect
              id="admin-map-availability"
              name="availability"
              className="native-select"
              value={filters.availability}
              disabled={busy !== null}
              onValueChange={(value) => select({ availability: value })}
              options={['active', 'withdrawn', 'all'].map((value) => ({
                value, label: value === 'all' ? c('all') : availability(value),
              }))}
            />
          </div>
          {filtered && (
            <Button
              type="button"
              variant="ghost"
              disabled={busy !== null}
              onClick={() =>
                select({ q: '', mode: 'all', source: 'all', status: 'all', availability: 'active' })
              }
            >
              {t('clearFilters')}
            </Button>
          )}
        </div>
        <Feedback error={searchError} />
      </form>
      <Feedback success={feedback} />
      {stale && (
        <div className="admin-map-summary" role="status">
          <p className="hint">{t('refreshFailed')}</p>
          <Button
            type="button"
            variant="outline"
            disabled={refreshing}
            onClick={() => refreshList()}
          >
            {refreshing && <LoaderCircle className="spin" aria-hidden="true" />}
            {t('refreshRetry')}
          </Button>
        </div>
      )}
      <section
        className="glass content-panel"
        aria-label={t('title')}
        aria-busy={isLoading || refreshing}
      >
        <div className="admin-map-summary">
          {data && <p role="status">{t('results', { count: data.total })}</p>}
          <p className="hint">{canModerate ? availability('actionHint') : a('actionsReadOnly')}</p>
        </div>
        {isLoading ? (
          <Loading />
        ) : error && !data ? (
          <>
            <Feedback error={describeError(error)} />
            <LoadError retry={() => mutate()} />
          </>
        ) : !data?.items.length ? (
          <Empty title={t(filtered ? 'noResults' : 'empty')}>
            <p>{t(filtered ? 'noResultsHint' : 'emptyHint')}</p>
          </Empty>
        ) : (
          <>
            <p className="hint admin-scroll-hint" id="admin-map-scroll-hint">
              {t('scrollHint')}
            </p>
            <div
              className="table-scroll admin-map-scroll"
              role="region"
              aria-label={a('beatmaps')}
              aria-describedby="admin-map-scroll-hint"
              tabIndex={0}
            >
              <table className="data-table admin-map-table">
                <thead>
                  <tr>
                    {[
                      a('beatmaps'),
                      c('mode'),
                      t('source'),
                      c('status'),
                      availability('label'),
                      a('action'),
                    ].map((label) => (
                      <th scope="col" key={label}>
                        {label}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {data.items.map((map) => (
                    <MapModerationRow
                      key={map.id}
                      map={map}
                      returnTo={returnTo}
                      busy={busy === map.id}
                      disabled={busy !== null || stale}
                      save={save}
                      canModerate={canModerate}
                      changed={refreshList}
                      onBusy={(writing) => setBusy(writing ? map.id : null)}
                    />
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </section>
      {data && (
        <Pagination
          page={filters.page}
          total={data.total}
          pageSize={data.page_size}
          onChange={(page) => select({ page })}
        />
      )}
    </div>
  );
}

function AdminContents() {
  const t = useTranslations('admin');
  const c = useTranslations('common');
  const f = useFormatter();
  const query = useSearchParams();
  const { user, refresh } = useSession();
  const { mutate: refreshCache, cache } = useSWRConfig();
  const describeError = useAdminError();
  const permissions = user?.permissions ?? [];
  const allowed = (p: string) => hasPermission(user?.permissions ?? [], p);
  const sections: Section[] = [];
  if (['users.moderate', 'roles.manage', 'users.sessions.revoke'].some(allowed))
    sections.push('users');
  if (allowed('roles.manage')) sections.push('roles');
  if (allowed('admin.view')) sections.push('beatmaps');
  if (allowed('scores.moderate')) sections.push('scores');
  if (allowed('audit.read')) sections.push('audit');
  if (allowed('announcements.manage')) sections.push('announcements');
  const requestedSection = query.get('tab') as Section | null;
  const section =
    requestedSection && sections.includes(requestedSection) ? requestedSection : sections[0];
  const [page, setPage] = useState(1);
  const [searchInput, setSearchInput] = useState('');
  const [search, setSearch] = useState('');
  const announcementNavigationGuard = useRef<AnnouncementNavigationGuard | null>(null);
  const registerAnnouncementNavigationGuard = useCallback(
    (guard: AnnouncementNavigationGuard | null) => {
      announcementNavigationGuard.current = guard;
    },
    [],
  );
  const key =
    section === 'scores'
      ? `/scores?verification=pending&page=${page}&page_size=24`
      : section
        ? `/admin/${section}?page=${page}&page_size=24${section === 'users' && search ? `&search=${encodeURIComponent(search)}` : ''}`
        : null;
  const { data, error, isLoading, mutate } = useSWR<Page<AdminUser | Score | Audit>>(
    section === 'roles' || section === 'beatmaps' || section === 'announcements' ? null : key,
    fetcher,
    {
      keepPreviousData: false,
      // A decision can return to a page whose cached response predates the write.
      dedupingInterval: 0,
      onSuccess: (result, requestedKey) => {
        // Only a response for this collection and page can shrink its pagination.
        if (requestedKey !== key || result.page !== page || result.page_size < 1) return;
        const lastPage = Math.max(1, Math.ceil(result.total / result.page_size));
        setPage((current) => (current === result.page && current > lastPage ? lastPage : current));
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
              if (section === s) return;
              const navigate = () => {
                const next = new URLSearchParams(window.location.search);
                next.set('tab', s);
                next.delete('page');
                window.history.pushState(null, '', `?${next}`);
                setPage(1);
                setFailed('');
                setFeedback(false);
              };
              if (section === 'announcements' && announcementNavigationGuard.current)
                announcementNavigationGuard.current(navigate);
              else navigate();
            }}
          >
            {t(s)}
          </button>
        ))}
      </div>
      {section !== 'beatmaps' && section !== 'announcements' && (
        <Feedback error={failed} success={feedback ? t('saved') : ''} stale={stale} />
      )}
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
      {section === 'announcements' ? (
        <AdminAnnouncements registerNavigationGuard={registerAnnouncementNavigationGuard} />
      ) : section === 'beatmaps' ? (
        <MapModeration />
      ) : (
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
                                    {u.can_manage_roles &&
                                      rolesError &&
                                      allowed('roles.manage') && (
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
      )}
      {section !== 'roles' && section !== 'beatmaps' && section !== 'announcements' && data && (
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
        <Suspense fallback={<Loading />}>
          <AdminContents />
        </Suspense>
      </Protected>
    </div>
  );
}
