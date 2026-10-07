'use client';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import Link from 'next/link';
import useSWR from 'swr';
import { useFormatter, useTranslations } from 'next-intl';
import { Download, LoaderCircle, Upload } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Loading, LoadError, Avatar } from '@/components/common';
import { useSession } from '@/components/providers';
import { api, ApiError, fetcher } from '@/lib/api';
import { scoreMods, scoreVerificationKey } from '@/lib/score-display';
import type { Beatmap, User, Score } from '@/lib/types';
type ScoreDetail = Score & {
  user: User;
  beatmap: Beatmap;
  hit_counts: Record<string, number>;
  client: string;
  accuracy_source: string;
  replay_url: string | null;
};
type ReplayAttachmentResult = {
  ok: boolean;
  has_replay: true;
  verification: string;
  idempotent?: boolean;
};

function isScoreAccessError(error: unknown): error is ApiError {
  return error instanceof ApiError && [401, 403, 404].includes(error.status);
}

function replayErrorKey(error: unknown) {
  if (!(error instanceof ApiError)) return 'errorUpload';
  if (error.status === 401) return 'errorSession';
  if (error.status === 403) return 'errorOwner';
  if (error.status === 404) return 'errorMissing';
  if (error.status === 413 || error.message === 'Replay exceeds 16 MiB.') return 'errorTooLarge';
  if (error.code === 'upload_busy' || error.status === 429) return 'errorBusy';
  if (error.code === 'replay_already_uploaded') return 'errorAlreadyAttached';
  if (error.code === 'replay_binding_changed' || error.code === 'replay_changed_retry')
    return 'errorChanged';
  if (error.message === 'Replay metadata does not match the stored score.') return 'errorMismatch';
  if (error.message === 'Stored lazer submission is required.') return 'errorLazerMetadata';
  if (error.message.startsWith('Invalid extended replay:')) return 'errorInvalidLazer';
  if (error.message.startsWith('Invalid replay container:')) return 'errorInvalidReplay';
  if (
    ['Upload an .osr replay.', 'Attach a replay file.', 'Attach exactly one replay file.'].includes(
      error.message,
    )
  )
    return 'errorFile';
  if (error.status === 400) return 'errorInvalidReplay';
  return 'errorUpload';
}

function ReplayAttachment({
  score,
  onAttached,
  onRefresh,
}: {
  score: ScoreDetail;
  onAttached: (result: ReplayAttachmentResult) => Promise<void>;
  onRefresh: () => Promise<ScoreDetail>;
}) {
  const t = useTranslations('scores');
  const c = useTranslations('common');
  const [file, setFile] = useState<File | null>(null);
  const [operation, setOperation] = useState<'upload' | 'refresh' | null>(null);
  const [errorKey, setErrorKey] = useState<ReturnType<typeof replayErrorKey> | null>(null);
  const [accepted, setAccepted] = useState<ReplayAttachmentResult | null>(null);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const operationRef = useRef(false);
  const alive = useRef(true);
  const fileRef = useRef<HTMLInputElement>(null);
  const feedbackRef = useRef<HTMLParagraphElement>(null);
  const refreshErrorRef = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (refreshFailed) refreshErrorRef.current?.focus();
    else if (errorKey || accepted) feedbackRef.current?.focus();
  }, [errorKey, accepted, refreshFailed]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (operationRef.current || accepted || score.has_replay) return;
    setErrorKey(null);
    setRefreshFailed(false);
    if (!file || file.size === 0 || !/\.osr$/i.test(file.name)) {
      setErrorKey('errorFile');
      return;
    }
    if (file.size > 16 * 1024 * 1024) {
      setErrorKey('errorTooLarge');
      return;
    }
    const data = new FormData();
    data.set('file', file);
    operationRef.current = true;
    setOperation('upload');
    try {
      const result = await api<ReplayAttachmentResult>(`/scores/${score.id}/replay`, {
        method: 'POST',
        body: data,
      });
      if (!alive.current) return;
      setAccepted(result);
      setFile(null);
      if (fileRef.current) fileRef.current.value = '';
      setOperation('refresh');
      try {
        await onAttached(result);
      } catch {
        if (alive.current) setRefreshFailed(true);
      }
    } catch (error) {
      if (alive.current) setErrorKey(replayErrorKey(error));
    } finally {
      operationRef.current = false;
      if (alive.current) setOperation(null);
    }
  }

  async function refresh() {
    if (operationRef.current) return;
    operationRef.current = true;
    setOperation('refresh');
    setRefreshFailed(false);
    try {
      await onRefresh();
      if (alive.current) setErrorKey(null);
    } catch {
      if (alive.current) setRefreshFailed(true);
    } finally {
      operationRef.current = false;
      if (alive.current) setOperation(null);
    }
  }

  if (score.has_replay && !accepted && !operation && !errorKey && !refreshFailed) return null;
  const busy = operation !== null;
  return (
    <section
      className="glass content-panel score-replay-upload"
      aria-labelledby="score-replay-title"
      aria-busy={busy}
    >
      <h2 id="score-replay-title">{t('replayTitle')}</h2>
      {accepted ? (
        <p className="form-success" role="status" ref={feedbackRef} tabIndex={-1}>
          {t(accepted.idempotent ? 'replayAlreadyAttached' : 'replayAttached')}
        </p>
      ) : (
        <form className="score-replay-form" onSubmit={submit} noValidate>
          <p className="hint" id="score-replay-hint">
            {t('replayHint')}
          </p>
          <label htmlFor="score-replay-file">{t('replayFile')}</label>
          <Input
            ref={fileRef}
            id="score-replay-file"
            name="file"
            type="file"
            accept=".osr"
            disabled={busy || score.has_replay}
            aria-invalid={Boolean(errorKey)}
            aria-describedby={`score-replay-hint${errorKey ? ' score-replay-error' : ''}`}
            onChange={(event) => {
              if (operationRef.current) return;
              setFile(event.currentTarget.files?.[0] ?? null);
              setErrorKey(null);
              setRefreshFailed(false);
            }}
          />
          {errorKey && (
            <p
              className="form-error"
              id="score-replay-error"
              role="alert"
              ref={feedbackRef}
              tabIndex={-1}
            >
              {t(errorKey)}
            </p>
          )}
          {(errorKey === 'errorSession' || errorKey === 'errorOwner') && (
            <Link href="/login" className="text-link">
              {c('signInRequired')}
            </Link>
          )}
          <div className="actions">
            <Button type="submit" disabled={busy || score.has_replay}>
              {operation === 'upload' ? <LoaderCircle className="animate-spin" /> : <Upload />}
              {t(operation === 'upload' ? 'replayUploading' : 'replayAttach')}
            </Button>
            {errorKey && (
              <Button type="button" variant="outline" onClick={refresh} disabled={busy}>
                {t(operation === 'refresh' ? 'replayRefreshing' : 'replayRefresh')}
              </Button>
            )}
          </div>
        </form>
      )}
      {refreshFailed && (
        <p className="form-error" role="alert" ref={refreshErrorRef} tabIndex={-1}>
          {t(accepted ? 'replayRefreshFailed' : 'scoreRefreshFailed')}
        </p>
      )}
      {accepted && (refreshFailed || operation === 'refresh') && (
        <Button type="button" variant="outline" onClick={refresh} disabled={busy}>
          {t(operation === 'refresh' ? 'replayRefreshing' : 'replayRefresh')}
        </Button>
      )}
    </section>
  );
}

export function ScorePage({ id }: { id: number }) {
  const c = useTranslations('common');
  const p = useTranslations('profile');
  const t = useTranslations('scores');
  const { user, loading: sessionLoading } = useSession();
  const f = useFormatter();
  const account = user?.id ?? 'guest';
  const scope = `${id}:${account}`;
  const [accessFailure, setAccessFailure] = useState<{ scope: string; error: ApiError } | null>(
    null,
  );
  const {
    data: score,
    error,
    isLoading,
    mutate,
  } = useSWR<ScoreDetail>(
    sessionLoading ? null : [`/scores/${id}`, account],
    ([path]: [string, number | string]) => fetcher<ScoreDetail>(path),
    {
      keepPreviousData: false,
      shouldRetryOnError: (requestError) => !isScoreAccessError(requestError),
      onSuccess: () => {
        setAccessFailure((current) => (current?.scope === scope ? null : current));
      },
      onError: (requestError) => {
        if (!isScoreAccessError(requestError)) return;
        setAccessFailure({ scope, error: requestError });
        // A policy denial invalidates this account's cached snapshot too.
        void mutate(undefined, { revalidate: false });
      },
    },
  );
  const denied =
    accessFailure?.scope === scope ? accessFailure.error : isScoreAccessError(error) ? error : null;

  async function refreshScore() {
    try {
      const updated = await fetcher<ScoreDetail>(`/scores/${id}`);
      await mutate(updated, { revalidate: false });
      setAccessFailure((current) => (current?.scope === scope ? null : current));
      return updated;
    } catch (refreshError) {
      if (isScoreAccessError(refreshError)) {
        setAccessFailure({ scope, error: refreshError });
        await mutate(undefined, { revalidate: false });
      }
      throw refreshError;
    }
  }
  async function attached(result: ReplayAttachmentResult) {
    await mutate(
      (current) =>
        current && {
          ...current,
          has_replay: true,
          replay_url: `/api/v1/scores/${id}/replay`,
          verification: result.verification,
          replay_complete: result.idempotent ? current.replay_complete : null,
        },
      { revalidate: false },
    );
    await refreshScore();
  }
  function retryScore() {
    setAccessFailure((current) => (current?.scope === scope ? null : current));
    void mutate();
  }
  if (sessionLoading || isLoading)
    return (
      <div className="page-container">
        <Loading />
      </div>
    );
  if (denied || (error && !score))
    return (
      <div className="page-container">
        <LoadError retry={retryScore} />
        {denied && (denied.status === 401 || denied.status === 403) && (
          <Link href="/login" className="text-link">
            {c('signInRequired')}
          </Link>
        )}
      </div>
    );
  if (!score) return null;
  const selection = `mode=${score.mode}&variant=${score.score_variant ?? 'vanilla'}`;
  const mods = scoreMods(score);
  const modText =
    [
      ...mods.labels,
      ...(mods.unknownBits ? [t('modsUnknown', { bits: mods.unknownBits })] : []),
    ].join(' ') || 'NM';
  const verificationKey = scoreVerificationKey(score.verification);
  const modSettings = (score.api_mods ?? []).flatMap((mod) =>
    Object.entries(mod.settings ?? {})
      .filter(([, value]) => value !== null)
      .map(([key, value]) => ({ mod: mod.acronym, key, value })),
  );
  return (
    <div className="page-container">
      <Link className="back-link" href={`/players/${score.user_id}?${selection}`}>
        {c('back')}
      </Link>
      <header className="score-detail-header glass">
        <div>
          <Link href={`/beatmaps/${score.beatmap_id}?${selection}`}>
            <h1>{score.title}</h1>
          </Link>
          <p>
            {score.version} · {score.beatmap.artist}
          </p>
          <Link href={`/players/${score.user_id}?${selection}`} className="player-cell">
            <Avatar name={score.username} url={score.user.avatar_url} />
            {score.username}
          </Link>
        </div>
        <div className="score-performance">
          <strong>
            {score.pp.toFixed(2)} <span>pp</span>
          </strong>
          <Badge variant="secondary">
            {verificationKey ? t(verificationKey) : score.verification}
          </Badge>
          {score.score_variant === 'relax' && <Badge variant="secondary">{c('relax')}</Badge>}
          {score.has_replay && (
            <Button asChild>
              <a href={`/api/v1/scores/${id}/replay`}>
                <Download />
                {c('replay')}
              </a>
            </Button>
          )}
        </div>
      </header>
      <dl className="score-detail-values glass">
        {[
          [p('scoreRaw'), f.number(score.score)],
          [c('accuracy'), `${score.accuracy.toFixed(2)}%`],
          [c('combo'), `${score.max_combo}x`],
          [c('mods'), modText],
          [
            c('date'),
            f.dateTime(new Date(score.created_at), { dateStyle: 'medium', timeStyle: 'short' }),
          ],
        ].map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      {user?.id === score.user_id && (
        <ReplayAttachment
          key={`${id}:${user.id}`}
          score={score}
          onAttached={attached}
          onRefresh={refreshScore}
        />
      )}
      {score.score_scale_status && (
        <section className="glass content-panel score-judgements mb-6">
          <h2>{p('scoreScales')}</h2>
          {score.score_scale_status !== 'unavailable' && (
            <dl>
              {[
                [p('scoreStandardised'), score.standardised_score],
                [p('scoreClassic'), score.classic_score],
              ].map(([label, value]) => (
                <div key={String(label)}>
                  <dt>{label}</dt>
                  <dd>{typeof value === 'number' ? f.number(value) : '—'}</dd>
                </div>
              ))}
            </dl>
          )}
          <p className="hint">
            {p(
              {
                native_reported: 'scoreNativeReported',
                legacy_estimated: 'scoreLegacyEstimated',
                legacy_scorev2: 'scoreLegacyScoreV2',
                unavailable: 'scoreScaleUnavailable',
              }[score.score_scale_status],
            )}
          </p>
        </section>
      )}
      {modSettings.length > 0 && (
        <section className="glass content-panel score-judgements mb-6">
          <h2>{c('modSettings')}</h2>
          <dl>
            {modSettings.map(({ mod, key, value }) => (
              <div key={`${mod}-${key}`}>
                <dt>
                  {mod} ·{' '}
                  {c.has(`modSettingsLabels.${key}`)
                    ? c(`modSettingsLabels.${key}`)
                    : key.replaceAll('_', ' ')}
                </dt>
                <dd>
                  {typeof value === 'number'
                    ? f.number(value)
                    : typeof value === 'boolean'
                      ? c(value ? 'enabled' : 'disabled')
                      : value}
                </dd>
              </div>
            ))}
          </dl>
        </section>
      )}
      <section className="glass content-panel score-judgements">
        <h2>{c('judgements')}</h2>
        <dl>
          {Object.entries(score.hit_counts).map(([key, value]) => (
            <div key={key}>
              <dt>{key.replace(/^n/, '').replaceAll('_', ' ')}</dt>
              <dd>{f.number(value)}</dd>
            </div>
          ))}
        </dl>
        <p className="hint">
          {score.verification === 'rejected'
            ? t('rejectedHint')
            : score.verification === 'verified'
              ? p('scoreVerified')
              : score.verification === 'pending'
                ? p('scorePending')
                : score.verification}
        </p>
        {score.pp_eligible === false && <p className="hint">{p('scorePpExcluded')}</p>}
        {score.has_replay && score.replay_complete === false && (
          <p className="hint">{p('scoreReplayIncomplete')}</p>
        )}
      </section>
    </div>
  );
}
