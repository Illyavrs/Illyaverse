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
  const mods = legacyMods.filter(([bit]) => (score.mods & bit) !== 0).map(([, label]) => label);
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
          [c('score'), f.number(score.score)],
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
      </section>
    </div>
  );
}
