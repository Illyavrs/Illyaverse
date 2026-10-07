'use client';
import Link from 'next/link';
import useSWR from 'swr';
import { useFormatter, useTranslations } from 'next-intl';
import { Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Loading, LoadError, Avatar } from '@/components/common';
import { fetcher } from '@/lib/api';
import type { Beatmap, User, Score } from '@/lib/types';
type ScoreDetail = Score & {
  user: User;
  beatmap: Beatmap;
  hit_counts: Record<string, number>;
  client: string;
  accuracy_source: string;
  replay_url: string | null;
};
const legacyMods: [[number, string], ...Array<[number, string]>] = [
  [1, 'NF'],
  [2, 'EZ'],
  [8, 'HD'],
  [16, 'HR'],
  [32, 'SD'],
  [64, 'DT'],
  [128, 'RX'],
  [256, 'HT'],
  [512, 'NC'],
  [1024, 'FL'],
  [2048, 'AT'],
  [4096, 'SO'],
  [8192, 'AP'],
  [16384, 'PF'],
];
export function ScorePage({ id }: { id: number }) {
  const c = useTranslations('common');
  const p = useTranslations('profile');
  const f = useFormatter();
  const { data: score, error, isLoading, mutate } = useSWR<ScoreDetail>(`/scores/${id}`, fetcher);
  if (isLoading)
    return (
      <div className="page-container">
        <Loading />
      </div>
    );
  if (error)
    return (
      <div className="page-container">
        <LoadError retry={() => mutate()} />
      </div>
    );
  if (!score) return null;
  const mods = score.api_mods
    ? score.api_mods.map((mod) => mod.acronym)
    : legacyMods
        .filter(([bit]) => (score.mods & bit) !== 0)
        .map(([, label]) => label)
        .filter(
          (label) =>
            !(label === 'DT' && score.mods & 512) && !(label === 'SD' && score.mods & 16384),
        );
  const modSettings = (score.api_mods ?? []).flatMap((mod) =>
    Object.entries(mod.settings ?? {})
      .filter(([, value]) => value !== null)
      .map(([key, value]) => ({ mod: mod.acronym, key, value })),
  );
  return (
    <div className="page-container">
      <Link className="back-link" href={`/players/${score.user_id}`}>
        {c('back')}
      </Link>
      <header className="score-detail-header glass">
        <div>
          <Link href={`/beatmaps/${score.beatmap_id}`}>
            <h1>{score.title}</h1>
          </Link>
          <p>
            {score.version} · {score.beatmap.artist}
          </p>
          <Link href={`/players/${score.user_id}`} className="player-cell">
            <Avatar name={score.username} url={score.user.avatar_url} />
            {score.username}
          </Link>
        </div>
        <div className="score-performance">
          <strong>
            {score.pp.toFixed(2)} <span>pp</span>
          </strong>
          <Badge variant="secondary">{score.verification}</Badge>
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
          [c('mods'), mods.join(' ') || 'NM'],
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
          {score.verification === 'verified' ? p('scoreVerified') : p('scorePending')}
        </p>
        {score.pp_eligible === false && <p className="hint">{p('scorePpExcluded')}</p>}
        {score.has_replay && score.replay_complete === false && (
          <p className="hint">{p('scoreReplayIncomplete')}</p>
        )}
      </section>
    </div>
  );
}
