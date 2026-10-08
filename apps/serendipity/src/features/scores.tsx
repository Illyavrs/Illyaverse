'use client';
import { useFormatter, useTranslations } from 'next-intl';
import Link from 'next/link';
import { ArrowUpRight, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Avatar } from '@/components/common';
import type { Score, ScoreBoardOrder } from '@/lib/types';
import { scoreMods, scoreVerificationKey } from '@/lib/score-display';
import './score-presentation.css';
export function ScoreTable({
  scores,
  leaderboard = false,
  orderedBy,
  profileBest = false,
}: {
  scores: Score[];
  leaderboard?: boolean;
  orderedBy?: ScoreBoardOrder;
  profileBest?: boolean;
}) {
  const c = useTranslations('common');
  const p = useTranslations('profile');
  const t = useTranslations('scores');
  const f = useFormatter();
  return (
    <div
      className="table-scroll score-record-table"
      tabIndex={0}
      role="region"
      aria-label={profileBest ? p('best') : c('tableScroll')}
    >
      <table className={`data-table score-table${profileBest ? ' profile-best-score-table' : ''}`}>
        <thead>
          <tr>
            {(leaderboard || profileBest) && (
              <th scope="col" className="score-record-number">
                {t('rankPosition')}
              </th>
            )}
            <th scope="col" className="score-record-identity-heading">
              {leaderboard ? c('username') : c('score')}
            </th>
            <th scope="col" className="score-record-primary-heading">
              {profileBest
                ? c('pp')
                : leaderboard
                  ? orderedBy === 'pp'
                    ? t('comparisonPp')
                    : t('comparisonStandardised')
                  : p('scoreRaw')}
            </th>
            {profileBest && (
              <>
                <th scope="col" className="score-record-number">
                  {p('ppWeight')}
                </th>
                <th scope="col" className="score-record-number">
                  {p('weightedPp')}
                </th>
              </>
            )}
            <th scope="col">{c('mods')}</th>
            <th scope="col" className="score-record-number">
              {c('accuracy')}
            </th>
            <th scope="col" className="score-record-number">
              {c('combo')}
            </th>
            {!profileBest && (!leaderboard || orderedBy !== 'pp') && (
              <th scope="col" className="score-record-number">
                pp
              </th>
            )}
            <th scope="col">{p('verification')}</th>
            <th scope="col" className="score-record-replay">
              {c('replay')}
            </th>
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
              <tr key={score.id} data-verification={score.verification}>
                {(leaderboard || profileBest) && (
                  <td className="score-board-position">
                    {typeof score.position === 'number' ? f.number(score.position) : '—'}
                  </td>
                )}
                <td className="score-record-identity">
                  {leaderboard && (
                    <Link
                      className="player-cell score-record-player"
                      href={`/players/${score.user_id}?mode=${score.mode}&variant=${score.score_variant ?? 'vanilla'}`}
                    >
                      <span aria-hidden="true" className="contents">
                        <Avatar name={score.username} />
                      </span>
                      <span>{score.username}</span>
                    </Link>
                  )}
                  <Link href={`/scores/${score.id}`} className="score-title">
                    <span className="score-record-title-line">
                      {score.title}
                      <ArrowUpRight aria-hidden="true" />
                    </span>
                    <span className="score-record-version">{score.version}</span>
                  </Link>
                  <div className="score-record-meta">
                    {score.score_variant === 'relax' && (
                      <Badge variant="secondary">{c('relax')}</Badge>
                    )}
                    <time dateTime={score.created_at}>
                      {f.dateTime(new Date(score.created_at), { dateStyle: 'medium' })}
                    </time>
                    {(leaderboard || profileBest) && (
                      <span title={p('scoreRaw')}>{f.number(score.score)}</span>
                    )}
                  </div>
                </td>
                <td
                  className={`score-record-primary${leaderboard ? ' score-board-comparison' : ' pp-value'}`}
                >
                  {profileBest
                    ? f.number(score.pp, { minimumFractionDigits: 2, maximumFractionDigits: 2 })
                    : leaderboard
                      ? orderedBy === 'pp'
                        ? score.pp.toFixed(2)
                        : typeof score.standardised_score === 'number'
                          ? f.number(score.standardised_score)
                          : '—'
                      : f.number(score.score)}
                </td>
                {profileBest && (
                  <>
                    <td className="score-weight">
                      {typeof score.pp_weight === 'number'
                        ? f.number(score.pp_weight, {
                            style: 'percent',
                            minimumFractionDigits: 2,
                            maximumFractionDigits: 2,
                          })
                        : '—'}
                    </td>
                    <td className="pp-value score-weighted-pp">
                      {typeof score.weighted_pp === 'number'
                        ? f.number(score.weighted_pp, {
                            minimumFractionDigits: 2,
                            maximumFractionDigits: 2,
                          })
                        : '—'}
                    </td>
                  </>
                )}
                <td className="score-record-mods" title={modText}>
                  <span className="score-mod-labels">{modText}</span>
                </td>
                <td className="score-record-number">
                  {profileBest
                    ? f.number(score.accuracy / 100, {
                        style: 'percent',
                        minimumFractionDigits: 2,
                        maximumFractionDigits: 2,
                      })
                    : `${score.accuracy.toFixed(2)}%`}
                </td>
                <td className="score-record-number">
                  {profileBest ? f.number(score.max_combo) : score.max_combo}x
                </td>
                {!profileBest && (!leaderboard || orderedBy !== 'pp') && (
                  <td className="pp-value score-record-number">{score.pp.toFixed(2)}</td>
                )}
                <td>
                  <Badge
                    variant="secondary"
                    className="score-record-verification"
                    data-verification={score.verification}
                  >
                    {verificationKey ? t(verificationKey) : score.verification}
                  </Badge>
                </td>
                <td className="score-record-replay">
                  {score.has_replay ? (
                    <Button
                      asChild
                      variant="ghost"
                      size="icon"
                      aria-label={p('downloadReplay', { title: score.title })}
                    >
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
