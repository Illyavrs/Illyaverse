'use client';
import { useFormatter, useTranslations } from 'next-intl';
import Link from 'next/link';
import { Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import type { Score } from '@/lib/types';
export function ScoreTable({ scores, leaderboard = false }: { scores: Score[]; leaderboard?: boolean }) {
  const c = useTranslations('common');
  const p = useTranslations('profile');
  const f = useFormatter();
  return (
    <div className="table-scroll">
      <table className="data-table score-table">
        <thead>
          <tr>
            <th>{c('score')}</th>
            <th>{c('accuracy')}</th>
            <th>{c('combo')}</th>
            <th>pp</th>
            <th>{p('verification')}</th>
            <th>{c('replay')}</th>
          </tr>
        </thead>
        <tbody>
          {scores.map((score) => (
            <tr key={score.id}>
              <td>
                {leaderboard && <Link className="player-cell" href={`/players/${score.user_id}?mode=${score.mode}&variant=${score.score_variant ?? 'vanilla'}`}>{score.username}</Link>}
                <Link href={`/scores/${score.id}`} className="score-title">
                  {score.title}
                  <span>
                    {score.version} <span className="dot">·</span> {f.number(score.score)}
                  </span>
                </Link>
                <div className="flex flex-wrap items-center gap-2">
                  {score.score_variant === 'relax' && <Badge variant="secondary">{c('relax')}</Badge>}
                  <small>{f.dateTime(new Date(score.created_at), { dateStyle: 'medium' })}</small>
                </div>
              </td>
              <td>{score.accuracy.toFixed(2)}%</td>
              <td>{score.max_combo}x</td>
              <td className="pp-value">{score.pp.toFixed(2)}</td>
              <td>
                <Badge variant="secondary">{score.verification}</Badge>
              </td>
              <td>
                {score.has_replay ? (
                  <Button asChild variant="ghost" size="icon" aria-label={c('download')}>
                    <a href={`/api/v1/scores/${score.id}/replay`}>
                      <Download />
                    </a>
                  </Button>
                ) : (
                  <span className="muted">—</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
