'use client';
import Link from 'next/link';
import { useFormatter, useTranslations } from 'next-intl';
import { Download } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { scoreMods, scoreVerificationKey } from '@/lib/score-display';
import type { Score } from '@/lib/types';

export function ProfileScoreList({ scores, best }: { scores: Score[]; best: boolean }) {
  const c = useTranslations('common');
  const p = useTranslations('profile');
  const t = useTranslations('scores');
  const f = useFormatter();
  const decimal = (value: number) =>
    f.number(value, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  return (
    <ol
      className={`profile-score-list score-table${best ? ' profile-best-score-list' : ''}`}
      aria-label={p(best ? 'best' : 'scores')}
    >
      {scores.map((score) => {
        const mods = scoreMods(score);
        const modText =
          [
            ...mods.labels,
            ...(mods.unknownBits ? [t('modsUnknown', { bits: mods.unknownBits })] : []),
          ].join(' ') || 'NM';
        const verification = scoreVerificationKey(score.verification);
        return (
          <li className="profile-score-row" key={score.id}>
            {best && (
              <span className="profile-score-position" aria-label={t('rankPosition')}>
                {typeof score.position === 'number' ? f.number(score.position) : '—'}
              </span>
            )}
            <div className="profile-score-main">
              <Link href={`/scores/${score.id}`} className="score-title">
                <strong>{score.title}</strong>
                <span>{score.version}</span>
              </Link>
              <div className="profile-score-meta">
                <time dateTime={score.created_at}>
                  {f.dateTime(new Date(score.created_at), { dateStyle: 'medium' })}
                </time>
                <span>
                  {p('scoreRaw')}: {f.number(score.score)}
                </span>
                <Badge variant="secondary">
                  {verification ? t(verification) : score.verification}
                </Badge>
                {score.score_variant === 'relax' && <Badge variant="secondary">{c('relax')}</Badge>}
              </div>
              <dl className="profile-score-details">
                <div>
                  <dt>{c('mods')}</dt>
                  <dd>{modText}</dd>
                </div>
                <div>
                  <dt>{c('accuracy')}</dt>
                  <dd>{decimal(score.accuracy)}%</dd>
                </div>
                <div>
                  <dt>{c('combo')}</dt>
                  <dd>{f.number(score.max_combo)}x</dd>
                </div>
              </dl>
              {best && (
                <dl className="profile-score-contribution">
                  <div>
                    <dt>{p('ppWeight')}</dt>
                    <dd className="score-weight">
                      {typeof score.pp_weight === 'number'
                        ? f.number(score.pp_weight, {
                            style: 'percent',
                            minimumFractionDigits: 2,
                            maximumFractionDigits: 2,
                          })
                        : '—'}
                    </dd>
                  </div>
                  <div>
                    <dt>{p('weightedPp')}</dt>
                    <dd className="score-weighted-pp">
                      {typeof score.weighted_pp === 'number' ? decimal(score.weighted_pp) : '—'}
                    </dd>
                  </div>
                </dl>
              )}
            </div>
            <div className="profile-score-performance">
              <span className="pp-value">
                {decimal(score.pp)} <small>pp</small>
              </span>
              {score.pp_eligible === false && <small>{p('ppExcluded')}</small>}
              {score.has_replay ? (
                <Button asChild variant="ghost" size="icon">
                  <a
                    href={`/api/v1/scores/${score.id}/replay`}
                    aria-label={p('downloadReplay', { title: score.title })}
                  >
                    <Download />
                  </a>
                </Button>
              ) : (
                <span className="profile-replay-unavailable">{p('replayUnavailable')}</span>
              )}
              {score.has_replay && score.replay_complete === false && (
                <small>{p('scoreReplayIncomplete')}</small>
              )}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
