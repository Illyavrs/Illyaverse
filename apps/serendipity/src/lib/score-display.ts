import type { Score } from './types';

// Legacy bit positions are a presentation format. Native structured mods remain
// authoritative when present, including their unprojected acronyms and settings.
const legacyModLabels: [number, string][] = [
  [0, 'NF'],
  [1, 'EZ'],
  [2, 'TD'],
  [3, 'HD'],
  [4, 'HR'],
  [5, 'SD'],
  [6, 'DT'],
  [7, 'RX'],
  [8, 'HT'],
  [9, 'NC'],
  [10, 'FL'],
  [11, 'AT'],
  [12, 'SO'],
  [13, 'AP'],
  [14, 'PF'],
  [15, '4K'],
  [16, '5K'],
  [17, '6K'],
  [18, '7K'],
  [19, '8K'],
  [20, 'FI'],
  [21, 'RD'],
  [22, 'CN'],
  [23, 'TP'],
  [24, '9K'],
  [25, 'CO'],
  [26, '1K'],
  [27, '3K'],
  [28, '2K'],
  [29, 'SV2'],
  [30, 'MR'],
];

export function scoreMods(score: Pick<Score, 'mods' | 'api_mods'>) {
  if (score.api_mods != null)
    return { labels: score.api_mods.map((mod) => mod.acronym), unknownBits: null };
  if (!Number.isSafeInteger(score.mods) || score.mods < 0)
    return { labels: [], unknownBits: String(score.mods) };
  const bits = BigInt(score.mods);
  let remaining = bits;
  const labels = legacyModLabels.flatMap(([position, label]) => {
    const bit = BigInt(2 ** position);
    if ((bits & bit) === BigInt(0)) return [];
    remaining &= ~bit;
    if (
      (label === 'DT' && (bits & BigInt(512)) !== BigInt(0)) ||
      (label === 'SD' && (bits & BigInt(16384)) !== BigInt(0))
    )
      return [];
    return [label];
  });
  return { labels, unknownBits: remaining === BigInt(0) ? null : `0x${remaining.toString(16)}` };
}

export function scoreVerificationKey(verification: string) {
  switch (verification) {
    case 'pending':
      return 'verificationPending';
    case 'verified':
      return 'verificationVerified';
    case 'rejected':
      return 'verificationRejected';
    default:
      return null;
  }
}
