'use client';
import { useState } from 'react';
import Image from 'next/image';
import Link from 'next/link';
import { useFormatter, useTranslations } from 'next-intl';
import {
  ArrowUpRight,
  ChevronsDown,
  ChevronsUp,
  CirclePlay,
  Crosshair,
  Download,
  EyeOff,
  Feather,
  Flashlight,
  Hand,
  MoreVertical,
  Music2,
  Shield,
  Star,
  type LucideIcon,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { scoreMods, scoreVerificationKey } from '@/lib/score-display';
import type { Score } from '@/lib/types';

const modIcons: Record<string, LucideIcon> = {
  NF: Shield,
  EZ: Feather,
  HD: EyeOff,
  HR: Crosshair,
  DT: ChevronsUp,
  NC: Music2,
  RX: Hand,
  HT: ChevronsDown,
  FL: Flashlight,
};
const gradeLabels: Record<string, string> = {
  ssh: 'SS+',
  xh: 'SS+',
  ss: 'SS',
  x: 'SS',
  sh: 'S+',
  s: 'S',
  a: 'A',
  b: 'B',
  c: 'C',
  d: 'D',
  f: 'F',
};

function ScoreArtwork({ setId }: { setId?: number | null }) {
  const valid = Number.isSafeInteger(setId) && setId! > 0 && setId! < 2_000_000_000;
  const src = valid ? `https://assets.ppy.sh/beatmaps/${setId}/covers/cover.jpg` : null;
  const [failed, setFailed] = useState<string | null>(null);
  if (!src || failed === src) return null;
  return (
    <span className="profile-score-artwork" aria-hidden="true">
      <Image
        src={src}
        alt=""
        width={600}
        height={160}
        unoptimized
        loading="lazy"
        onError={() => setFailed(src)}
      />
    </span>
  );
}

function hitJudgements(score: Score) {
  const keys =
    score.mode === 3
      ? [
          ['ngeki', '320'],
          ['n300', '300'],
          ['nkatu', '200'],
          ['n100', '100'],
          ['n50', '50'],
          ['nmiss', 'miss'],
        ]
      : score.mode === 1
        ? [
            ['n300', '300'],
            ['n100', '100'],
            ['nmiss', 'miss'],
          ]
        : [
            ['n300', '300'],
            ['n100', '100'],
            ['n50', '50'],
            ['nmiss', 'miss'],
          ];
  return keys.flatMap(([key, label]) => {
    const value = score.hit_counts?.[key];
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
      ? [{ key, label, value }]
      : [];
  });
}

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
        const gradeKey = typeof score.grade === 'string' ? score.grade.toLowerCase() : '';
        const grade = gradeLabels[gradeKey];
        const stars =
          typeof score.stars === 'number' && Number.isFinite(score.stars) && score.stars >= 0
            ? decimal(score.stars)
            : null;
        const maximum =
          typeof score.beatmap_max_combo === 'number' &&
          Number.isSafeInteger(score.beatmap_max_combo) &&
          score.beatmap_max_combo > 0
            ? score.beatmap_max_combo
            : null;
        const judgements = hitJudgements(score);
        const date = new Date(score.created_at);
        return (
          <li
            className="profile-score-row glass"
            key={score.id}
            data-verification={score.verification}
            data-material="content"
          >
            <ScoreArtwork setId={score.beatmapset_id} />
            <div className="profile-score-leading">
              {best && (
                <span className="profile-score-position" aria-label={t('rankPosition')}>
                  {typeof score.position === 'number' ? f.number(score.position) : '—'}
                </span>
              )}
              {grade ? (
                <span
                  className="profile-score-grade"
                  data-grade={gradeKey}
                  aria-label={p('scoreGrade', { grade })}
                >
                  {grade}
                </span>
              ) : (
                <CirclePlay className="profile-score-symbol" aria-hidden="true" />
              )}
            </div>
            <div className="profile-score-main">
              <Link href={`/scores/${score.id}`} className="score-title">
                <strong>
                  {score.title}
                  {score.artist && <span className="profile-score-artist"> · {score.artist}</span>}
                </strong>
              </Link>
              <div className="profile-score-meta">
                {stars && (
                  <span
                    className="profile-score-stars"
                    data-tier={
                      score.stars! < 2
                        ? 'easy'
                        : score.stars! < 3
                          ? 'normal'
                          : score.stars! < 4
                            ? 'hard'
                            : score.stars! < 5
                              ? 'insane'
                              : score.stars! < 6
                                ? 'expert'
                                : 'extreme'
                    }
                    aria-label={p('scoreDifficulty', { stars })}
                  >
                    <Star aria-hidden="true" />
                    {stars}
                  </span>
                )}
                <span className="profile-score-version" title={score.version}>
                  {score.version}
                </span>
                <time
                  dateTime={score.created_at}
                  title={f.dateTime(date, { dateStyle: 'full', timeStyle: 'short' })}
                >
                  {f.dateTime(date, { dateStyle: 'medium' })}
                </time>
                {!score.passed && <span className="profile-score-failed">{h('failedScore')}</span>}
                {score.verification !== 'verified' && (
                  <span
                    className="profile-score-verification"
                    data-verification={score.verification}
                  >
                    {verification ? t(verification) : score.verification}
                  </span>
                )}
              </div>
            </div>
            <div className="profile-score-result">
              <div
                className="profile-score-mods"
                title={modText}
                aria-label={`${c('mods')}: ${modText}`}
              >
                {(mods.labels.length
                  ? mods.labels.slice(0, 3)
                  : !mods.unknownBits
                    ? ['NM']
                    : []
                ).map((label, index) => {
                  const Icon = modIcons[label];
                  return (
                    <span key={`${label}-${index}`} className="profile-score-mod" data-mod={label}>
                      {Icon && <Icon aria-hidden="true" />}
                      {label}
                    </span>
                  );
                })}
                {mods.labels.length > 3 && (
                  <span className="profile-score-mod">+{mods.labels.length - 3}</span>
                )}
                {mods.unknownBits && (
                  <span className="profile-score-mod">
                    {t('modsUnknown', { bits: mods.unknownBits })}
                  </span>
                )}
              </div>
              <div className="profile-score-statistics">
                <dl className="profile-score-details">
                  <div className="profile-score-accuracy">
                    <dt className="sr-only">{c('accuracy')}</dt>
                    <dd>
                      {decimal(score.accuracy)}
                      <small>%</small>
                    </dd>
                  </div>
                  <div className="profile-score-combo">
                    <dt className="sr-only">{c('combo')}</dt>
                    <dd>
                      {f.number(score.max_combo)}
                      {maximum && <span>/{f.number(maximum)}</span>}
                      <small>x</small>
                    </dd>
                  </div>
                </dl>
                {judgements.length > 0 && (
                  <dl className="profile-score-hits" aria-label={c('judgements')}>
                    {judgements.map(({ key, label, value }) => (
                      <div key={key} data-judgement={key}>
                        <dt>{label}</dt>
                        <dd>{f.number(value)}</dd>
                      </div>
                    ))}
                  </dl>
                )}
              </div>
              <div className="profile-score-performance">
                <Link href={`/scores/${score.id}`} className="pp-value">
                  {decimal(score.pp)}
                  <small>pp</small>
                </Link>
                {score.pp_eligible === false && <small>{p('ppExcluded')}</small>}
                {best && (
                  <dl className="profile-score-contribution">
                    <div>
                      <dt className="sr-only">{p('ppWeight')}</dt>
                      <dd className="score-weight" title={p('ppWeight')}>
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
                      <dt className="sr-only">{p('weightedPp')}</dt>
                      <dd className="score-weighted-pp" title={p('weightedPp')}>
                        {typeof score.weighted_pp === 'number' ? decimal(score.weighted_pp) : '—'}
                      </dd>
                    </div>
                  </dl>
                )}
              </div>
            </div>
            <div className="profile-score-replay">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={p('scoreActions', { title: score.title })}
                  >
                    <MoreVertical />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="profile-score-menu">
                  <DropdownMenuItem asChild>
                    <Link href={`/scores/${score.id}`}>
                      <ArrowUpRight />
                      {p('viewScore')}
                    </Link>
                  </DropdownMenuItem>
                  <DropdownMenuItem asChild={score.has_replay} disabled={!score.has_replay}>
                    {score.has_replay ? (
                      <a
                        href={`/api/v1/scores/${score.id}/replay`}
                        aria-label={p('downloadReplay', { title: score.title })}
                      >
                        <Download />
                        {c('replay')}
                      </a>
                    ) : (
                      <span>
                        <Download />
                        {p('replayUnavailable')}
                      </span>
                    )}
                  </DropdownMenuItem>
                  <dl className="profile-score-menu-facts">
                    <div>
                      <dt>{p('scoreRaw')}</dt>
                      <dd>{f.number(score.score)}</dd>
                    </div>
                    <div>
                      <dt>{c('mods')}</dt>
                      <dd>{modText}</dd>
                    </div>
                    <div>
                      <dt>{c('combo')}</dt>
                      <dd>
                        {f.number(score.max_combo)}
                        {maximum && `/${f.number(maximum)}`}x
                      </dd>
                    </div>
                    <div>
                      <dt>{c('date')}</dt>
                      <dd>{f.dateTime(date, { dateStyle: 'medium', timeStyle: 'short' })}</dd>
                    </div>
                    <div>
                      <dt>{p('verification')}</dt>
                      <dd
                        className="profile-score-verification"
                        data-verification={score.verification}
                      >
                        {verification ? t(verification) : score.verification}
                      </dd>
                    </div>
                  </dl>
                  {judgements.length > 0 && (
                    <dl className="profile-score-menu-hits" aria-label={c('judgements')}>
                      {judgements.map(({ key, label, value }) => (
                        <div key={key}>
                          <dt>{label}</dt>
                          <dd>{f.number(value)}</dd>
                        </div>
                      ))}
                    </dl>
                  )}
                  {score.has_replay && score.replay_complete === false && (
                    <p className="profile-score-menu-note">{p('scoreReplayIncomplete')}</p>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </li>
        );
      })}
    </ol>
  );
}
