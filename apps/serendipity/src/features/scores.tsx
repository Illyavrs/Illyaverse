'use client';
import { useFormatter, useTranslations } from 'next-intl';
import Link from 'next/link';
import { Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import type { Score, ScoreBoardOrder } from '@/lib/types';
import { scoreMods, scoreVerificationKey } from '@/lib/score-display';
export function ScoreTable({
  scores,
  leaderboard = false,
  orderedBy,
}: {
  scores: Score[];
  leaderboard?: boolean;
  orderedBy?: ScoreBoardOrder;
}) {
  const c = useTranslations('common');
  const p = useTranslations('profile');
  const t = useTranslations('scores');
  const f = useFormatter();
  return (
    <div className="table-scroll">
      <table className="data-table score-table">
        <thead>
          <tr>
            {leaderboard && <th scope="col">{t('rankPosition')}</th>}
            <th>{c('score')}</th>
            {leaderboard && (
              <th scope="col">
                {orderedBy === 'pp' ? t('comparisonPp') : t('comparisonStandardised')}
              </th>
            )}
            <th scope="col">{c('mods')}</th>
            <th>{c('accuracy')}</th>
            <th>{c('combo')}</th>
            {(!leaderboard || orderedBy !== 'pp') && <th>pp</th>}
            <th>{p('verification')}</th>
            <th>{c('replay')}</th>
          </tr>
        </thead>
        <tbody>
          {scores.map((score) => {
            const mods = scoreMods(score);
            const modText =
              [
                ...mods.labels,
                ...(mods.unknownBits ? [t('modsUnknown', { bits: mods.unknownBits })] : []),
              ].join(' ') || 'NM';
            const verificationKey = scoreVerificationKey(score.verification);
            return (
              <tr key={score.id}>
                {leaderboard && (
                  <td className="score-board-position">
                    {typeof score.position === 'number' ? f.number(score.position) : '—'}
                  </td>
                )}
                <td>
                  {leaderboard && (
                    <Link
                      className="player-cell"
                      href={`/players/${score.user_id}?mode=${score.mode}&variant=${score.score_variant ?? 'vanilla'}`}
                    >
                      {score.username}
                    </Link>
                  )}
                  <Link href={`/scores/${score.id}`} className="score-title">
                    {score.title}
                    <span>
                      {score.version} <span className="dot">·</span> {f.number(score.score)}
                    </span>
                  </Link>
                  <div className="flex flex-wrap items-center gap-2">
                    {score.score_variant === 'relax' && (
                      <Badge variant="secondary">{c('relax')}</Badge>
                    )}
                    <small>{f.dateTime(new Date(score.created_at), { dateStyle: 'medium' })}</small>
                  </div>
                </td>
                {leaderboard && (
                  <td className="score-board-comparison">
                    {orderedBy === 'pp'
                      ? score.pp.toFixed(2)
                      : typeof score.standardised_score === 'number'
                        ? f.number(score.standardised_score)
                        : '—'}
                  </td>
                )}
                <td>{modText}</td>
                <td>{score.accuracy.toFixed(2)}%</td>
                <td>{score.max_combo}x</td>
                {(!leaderboard || orderedBy !== 'pp') && (
                  <td className="pp-value">{score.pp.toFixed(2)}</td>
                )}
                <td>
                  <Badge variant="secondary">
                    {verificationKey ? t(verificationKey) : score.verification}
                  </Badge>
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
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
