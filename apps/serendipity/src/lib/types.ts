export type Page<T> = { items: T[]; total: number; page: number; page_size: number };
export type ScoreVariant = 'vanilla' | 'relax';
export type ScoreBoardOrder = 'standardised_score' | 'pp';
export type ScoreBoard = Page<Score> & {
  mode: number;
  score_variant: ScoreVariant;
  ordered_by: ScoreBoardOrder;
};
export type BestScorePage = Page<Score> & {
  collection: 'best';
  ordered_by: 'pp';
  mode: number;
  score_variant: ScoreVariant;
};
export type User = {
  id: number;
  username: string;
  country: string;
  bio: string;
  avatar_url: string | null;
  banner_url: string | null;
  banner_position_x: number;
  banner_position_y: number;
  accent_color: string;
  roles: string[];
  permissions: string[];
  joined_at: string;
  pp: number;
  play_count: number;
  statistics?: {
    mode: number;
    score_variant: ScoreVariant;
    pp: number;
    rank: number | null;
    country_rank: number | null;
    play_count: number;
    accuracy: number;
  };
  email?: string;
  email_verified?: boolean;
  is_restricted?: boolean;
};
export type Preferences = {
  mode: 'dark' | 'light' | 'system';
  accent_color: string;
  background_url: string | null;
  background_blur: number;
  background_dim: number;
  panel_blur: number;
  panel_opacity: number;
  border_radius: number;
  locale?: string;
  pm_friends_only?: boolean;
};
export type BeatmapDifficulty = {
  id: number;
  title: string;
  artist: string;
  creator: string;
  version: string;
  mode: number;
  status: string;
  source: string;
  stars: number;
  length_seconds: number;
};
export type BeatmapPackage = {
  id: string;
  native_set_id: number;
  kind: 'osu' | 'osz';
  legacy: boolean;
  difficulty_count: number;
  beatmap_ids: number[];
  download_url: string;
  source?: 'local' | 'official';
};
export type Beatmap = BeatmapDifficulty & {
  native_set_id?: number;
  availability?: BeatmapAvailability;
  official_id: number | null;
  bpm: number;
  max_combo: number;
  play_count: number;
  md5: string;
  download_url: string;
  download_kind: 'osu' | 'osz';
  package: BeatmapPackage | null;
};
export type BeatmapAvailability = {
  withdrawn: boolean;
  revision: number;
  reason: string | null;
  changed_at: string | null;
};
export type BeatmapUploadResult = {
  items: Beatmap[];
  package: BeatmapPackage;
  idempotent: boolean;
};
export type Score = {
  id: number;
  user_id: number;
  username: string;
  beatmap_id: number;
  title: string;
  version: string;
  mode: number;
  score_variant?: ScoreVariant;
  score: number;
  standardised_score?: number | null;
  standardised_without_mods?: number | null;
  classic_score?: number | null;
  score_scale_status?: 'native_reported' | 'legacy_estimated' | 'legacy_scorev2' | 'unavailable';
  accuracy: number;
  max_combo: number;
  mods: number;
  api_mods?:
    { acronym: string; settings?: Record<string, number | string | boolean | null> }[] | null;
  pp_eligible?: boolean;
  pp: number;
  passed: boolean;
  verification: string;
  created_at: string;
  has_replay: boolean;
  replay_complete?: boolean | null;
  position?: number;
  pp_weight?: number;
  weighted_pp?: number;
};
export type Channel = {
  id: number;
  name: string;
  description: string;
  required_permission?: string | null;
};
export type Message = {
  id: number;
  channel_id: number;
  user_id: number;
  username: string;
  avatar_url: string | null;
  content: string;
  created_at: string;
  is_action: boolean;
  client_uuid: string | null;
};
export type Conversation = {
  id: number;
  peer: { id: number; username: string; avatar_url: string | null };
  last_message: Message | null;
  last_read_id: number | null;
  unread_count: number;
  can_send: boolean;
  send_restriction: 'friend_only' | 'no_permission' | null;
};
export type Role = {
  id: number;
  name: string;
  permissions: string[];
  description: string;
  membership_count: number;
  is_builtin: boolean;
};
export type AdminUser = {
  id: number;
  username: string;
  email?: string;
  country: string;
  avatar_url: string | null;
  is_restricted: boolean;
  joined_at: string;
  roles: { id: number; name: string }[];
  can_manage_roles: boolean;
  can_moderate: boolean;
  can_revoke_sessions: boolean;
};
export type Leader = {
  id: number;
  username: string;
  country: string;
  avatar_url: string | null;
  pp: number;
  rank: number;
  play_count: number;
  accuracy: number;
};
export type Audit = {
  id: number;
  actor_id: number | null;
  action: string;
  target_type: string;
  target_id: string | null;
  created_at: string;
  details: Record<string, unknown>;
};
export type Status = {
  players_online: number;
  registered_players: number;
  ranked_scores: number;
  beatmap_count: number;
  compatibility: Record<string, unknown>;
};
