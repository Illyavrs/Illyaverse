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
  const h = useTranslations('home');
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
          <li className="profile-score-row" key={score.id} data-verification={score.verification}>
            {best && (
              <span className="profile-score-position" aria-label={t('rankPosition')}>
                {typeof score.position === 'number' ? f.number(score.position) : '—'}
              </span>
            )}
            <div className="profile-score-main">
              <Link href={`/scores/${score.id}`} className="score-title">
                <strong>
                  {!score.passed && (
                    <span
                      className="profile-score-failed"
                      title={h('failedScore')}
                      aria-label={h('failedScore')}
                    >
                      F
                    </span>
                  )}
                  {score.title}
                </strong>
                <span>{score.version}</span>
              </Link>
              <div className="profile-score-meta">
                <time dateTime={score.created_at}>
                  {f.dateTime(new Date(score.created_at), { dateStyle: 'medium' })}
                </time>
                <span>
                  {p('scoreRaw')}: {f.number(score.score)}
                </span>
                <span className="profile-score-verification" data-verification={score.verification}>
                  {verification ? t(verification) : score.verification}
                </span>
                {score.score_variant === 'relax' && <Badge variant="secondary">{c('relax')}</Badge>}
              </div>
              <dl className="profile-score-details">
                <div>
                  <dt>{c('mods')}</dt>
                  <dd className="profile-score-mods" title={modText}>
                    {mods.labels.length
                      ? mods.labels.map((label, index) => (
                          <span key={`${label}-${index}`}>
                            {label}
                            {index < mods.labels.length - 1 ? ' ' : ''}
                          </span>
                        ))
                      : !mods.unknownBits && <span>NM</span>}
                    {mods.unknownBits && (
                      <span>{t('modsUnknown', { bits: mods.unknownBits })}</span>
                    )}
                  </dd>
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
            </div>
            <div className="profile-score-performance">
              <Link href={`/scores/${score.id}`} className="pp-value">
                {decimal(score.pp)} <small>pp</small>
              </Link>
              {score.pp_eligible === false && <small>{p('ppExcluded')}</small>}
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
              {score.has_replay && score.replay_complete === false && (
                <small>{p('scoreReplayIncomplete')}</small>
              )}
            </div>
            <div className="profile-score-replay">
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
                <span className="profile-replay-unavailable" title={p('replayUnavailable')}>
                  <span aria-hidden="true">—</span>
                  <span className="sr-only">{p('replayUnavailable')}</span>
                </span>
              )}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
