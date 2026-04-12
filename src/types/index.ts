// ============================================
// Alexandria v2 — Shared TypeScript Types
// Generated from Rust domain structs (src-tauri/src/domain/)
// ============================================

// ---- Identity & Profile ----

export interface Identity {
  stake_address: string
  payment_address: string
  display_name: string | null
  bio: string | null
  avatar_cid: string | null
  profile_hash: string | null
  created_at: string
  updated_at: string
}

export interface WalletInfo {
  stake_address: string
  payment_address: string
  has_mnemonic_backup: boolean
}

export interface ProfileUpdate {
  display_name?: string | null
  bio?: string | null
  avatar_cid?: string | null
}

export interface PublishProfileResult {
  profile_hash: string
  profile: SignedProfile
}

export interface SignedProfile {
  version: number
  stake_address: string
  name?: string | null
  bio?: string | null
  avatar_hash?: string | null
  created_at: number
  updated_at: number
  signature: string
  public_key: string
}

// ---- Course ----

export interface Course {
  id: string
  title: string
  description: string | null
  author_address: string
  author_name: string | null
  content_cid: string | null
  thumbnail_cid: string | null
  thumbnail_svg: string | null
  tags: string[] | null
  skill_ids: string[] | null
  version: number
  status: string
  published_at: string | null
  on_chain_tx: string | null
  created_at: string
  updated_at: string
  /** `"course"` or `"tutorial"`. Older peers may omit this — defaults to `"course"` on the backend side. */
  kind: string
}

// ---- Tutorials (standalone video) ----

export interface VideoChapterInput {
  title: string
  start_seconds: number
}

export interface TutorialQuizInput {
  /** JSON body matching the existing quiz element format. */
  content_json: string
}

export interface SkillTagInput {
  skill_id: string
  /** Weight in [0.0, 1.0]. Defaults to 1.0 at the DB layer. */
  weight?: number | null
}

export interface PublishTutorialRequest {
  title: string
  description?: string | null
  /** BLAKE3 content hash of the uploaded video blob (via `content_add`). */
  video_content_hash: string
  thumbnail_hash?: string | null
  duration_seconds?: number | null
  skill_tags: SkillTagInput[]
  video_chapters?: VideoChapterInput[]
  quiz?: TutorialQuizInput | null
  tags?: string[]
}

export interface CreateCourseRequest {
  title: string
  description?: string | null
  tags?: string[] | null
  skill_ids?: string[] | null
}

export interface UpdateCourseRequest {
  title?: string | null
  description?: string | null
  tags?: string[] | null
  skill_ids?: string[] | null
  status?: string | null
}

export interface Chapter {
  id: string
  course_id: string
  title: string
  description: string | null
  position: number
}

export interface CreateChapterRequest {
  title: string
  description?: string | null
}

export interface UpdateChapterRequest {
  title?: string | null
  description?: string | null
  position?: number | null
}

export interface Element {
  id: string
  chapter_id: string
  title: string
  element_type: string
  content_cid: string | null
  content_inline: string | null
  position: number
  duration_seconds: number | null
}

export interface CreateElementRequest {
  title: string
  element_type: string
  content_hash?: string | null
  duration_seconds?: number | null
}

export interface UpdateElementRequest {
  title?: string | null
  element_type?: string | null
  content_hash?: string | null
  position?: number | null
  duration_seconds?: number | null
}

export interface PublishCourseResult {
  content_hash: string
  size: number
}

// ---- Enrollment ----

export interface Enrollment {
  id: string
  course_id: string
  enrolled_at: string
  completed_at: string | null
  status: string
  updated_at: string
}

export interface ElementProgress {
  id: string
  enrollment_id: string
  element_id: string
  status: string
  score: number | null
  time_spent: number
  completed_at: string | null
  updated_at: string
}

export interface UpdateProgressRequest {
  element_id: string
  status: string
  score?: number | null
  time_spent?: number | null
}

// ---- Evidence & Skill Proofs ----

export type ProficiencyLevel = 'remember' | 'understand' | 'apply' | 'analyze' | 'evaluate' | 'create'

export interface SkillAssessment {
  id: string
  skill_id: string
  course_id: string | null
  source_element_id: string | null
  assessment_type: string
  proficiency_level: string
  difficulty: number
  weight: number
  trust_factor: number
  created_at: string
}

export interface EvidenceRecord {
  id: string
  skill_assessment_id: string
  skill_id: string
  proficiency_level: string
  score: number
  difficulty: number
  trust_factor: number
  course_id: string | null
  instructor_address: string | null
  created_at: string
}

export interface SkillProof {
  id: string
  skill_id: string
  proficiency_level: string
  confidence: number
  evidence_count: number
  computed_at: string
  updated_at: string
  nft_policy_id: string | null
  nft_asset_name: string | null
  nft_tx_hash: string | null
}

export interface ReputationAssertion {
  id: string
  actor_address: string
  role: string
  skill_id: string | null
  proficiency_level: string | null
  score: number
  evidence_count: number
  computation_spec: string
  updated_at: string
}

// ---- Reputation ----

export type ReputationRole = 'instructor' | 'learner' | 'assessor' | 'author' | 'mentor'

export interface FullReputationAssertion {
  id: string
  actor_address: string
  role: string
  skill_id: string | null
  proficiency_level: string | null
  score: number
  confidence: number
  evidence_count: number
  distribution: DistributionMetrics | null
  computation_spec: string
  window_start: string | null
  window_end: string | null
  updated_at: string
}

export interface DistributionMetrics {
  median_impact: number
  impact_p25: number
  impact_p75: number
  learner_count: number
  impact_variance: number
}

export interface InstructorRanking {
  actor_address: string
  skill_id: string
  proficiency_level: string
  impact_score: number
  confidence: number
  learner_count: number
  median_impact: number
  rank: number
}

export interface ReputationQuery {
  actor_address?: string | null
  role?: string | null
  skill_id?: string | null
  proficiency_level?: string | null
  limit?: number | null
}

export interface RecomputeResult {
  assertions_updated: number
  deltas_recomputed: number
  duration_ms: number
}

export interface VerificationResult {
  score_matches: boolean
  confidence_matches: boolean
  recomputed_score: number
  recomputed_confidence: number
  claimed_score: number
  claimed_confidence: number
  max_diff: number
}

// ---- Snapshots ----

export type SnapshotStatus = 'pending' | 'building' | 'submitted' | 'confirmed' | 'failed'

export interface SnapshotRecord {
  id: string
  actor_address: string
  subject_id: string
  role: string
  skill_count: number
  tx_status: string
  tx_hash: string | null
  policy_id: string | null
  ref_asset_name: string | null
  user_asset_name: string | null
  error_message: string | null
  snapshot_at: string
  confirmed_at: string | null
}

export interface CreateSnapshotParams {
  subject_id: string
  role: string
}

// ---- Governance ----

export type ElectionPhase = 'nomination' | 'voting' | 'finalized' | 'cancelled'
export type ProposalStatus = 'draft' | 'published' | 'approved' | 'rejected' | 'cancelled'

export interface DaoInfo {
  id: string
  name: string
  description: string | null
  icon_emoji: string | null
  scope_type: string
  scope_id: string
  status: string
  committee_size: number
  election_interval_days: number
  on_chain_tx: string | null
  created_at: string
  updated_at: string
}

export interface DaoMember {
  dao_id: string
  stake_address: string
  role: string
  joined_at: string
}

export interface Election {
  id: string
  dao_id: string
  title: string
  description: string | null
  phase: string
  seats: number
  nominee_min_proficiency: string
  voter_min_proficiency: string
  nomination_start: string
  nomination_end: string | null
  voting_end: string | null
  on_chain_tx: string | null
  created_at: string
  finalized_at: string | null
}

export interface ElectionNominee {
  id: string
  election_id: string
  stake_address: string
  accepted: boolean
  votes_received: number
  is_winner: boolean
  nominated_at: string
}

export interface ElectionVote {
  id: string
  election_id: string
  voter: string
  nominee_id: string
  on_chain_tx: string | null
  voted_at: string
}

export interface OpenElectionParams {
  dao_id: string
  title: string
  description?: string | null
  seats?: number | null
  nominee_min_proficiency?: string | null
  voter_min_proficiency?: string | null
  nomination_end?: string | null
  voting_end?: string | null
}

export interface Proposal {
  id: string
  dao_id: string
  title: string
  description: string | null
  category: string
  status: string
  proposer: string
  votes_for: number
  votes_against: number
  voting_deadline: string | null
  min_vote_proficiency: string
  on_chain_tx: string | null
  created_at: string
  resolved_at: string | null
}

export interface ProposalVote {
  id: string
  proposal_id: string
  voter: string
  in_favor: boolean
  on_chain_tx: string | null
  voted_at: string
}

export interface SubmitProposalParams {
  dao_id: string
  title: string
  description?: string | null
  category: string
  min_vote_proficiency?: string | null
}

export interface GovernanceTxResult {
  tx_hash: string
  action: string
}

// ---- Catalog ----

export interface CatalogEntry {
  course_id: string
  title: string
  description: string | null
  author_address: string
  content_cid: string
  thumbnail_cid: string | null
  tags: string[] | null
  skill_ids: string[] | null
  version: number
  published_at: string
  received_at: string
  pinned: boolean
  on_chain_tx: string | null
  /** `"course"` or `"tutorial"`. Older announcements default to `"course"`. */
  kind: string
}

// ---- Taxonomy ----

export interface TaxonomySubjectField {
  id: string
  name: string
  description: string | null
}

export interface TaxonomySubject {
  id: string
  name: string
  description: string | null
  subject_field_id: string
}

export interface TaxonomySkill {
  id: string
  name: string
  description: string | null
  subject_id: string
  bloom_level: string
}

export interface TaxonomyChanges {
  subject_fields: TaxonomySubjectField[]
  subjects: TaxonomySubject[]
  skills: TaxonomySkill[]
  prerequisites: [string, string][]
  removed_prerequisites: [string, string][]
}

export interface TaxonomyVersion {
  version: number
  cid: string
  previous_cid: string | null
  ratified_by: string | null
  ratified_at: string | null
  signature: string | null
  applied_at: string
}

export interface TaxonomyPreview {
  subject_fields_affected: number
  subjects_affected: number
  skills_affected: number
  prerequisites_added: number
  prerequisites_removed: number
  has_modifications: boolean
  new_skill_ids: string[]
  modified_skill_ids: string[]
}

export interface TaxonomyPublishResult {
  version: number
  content_cid: string
  changes_applied: number
}

export interface ProposeTaxonomyParams {
  dao_id: string
  title: string
  description?: string | null
  changes: TaxonomyChanges
}

// ---- Sync ----

export interface DeviceInfo {
  id: string
  device_name: string | null
  platform: string | null
  first_seen: string
  last_synced: string | null
  is_local: boolean
  peer_id: string | null
}

export interface SyncStatus {
  device_count: number
  queue_length: number
  auto_sync: boolean
  last_sync: string | null
  devices: DeviceSyncSummary[]
}

export interface DeviceSyncSummary {
  device_id: string
  device_name: string | null
  last_synced: string | null
  tables_synced: number
  is_online: boolean
}

export interface SyncResult {
  rows_sent: number
  rows_received: number
  rows_merged: number
  table_stats: [string, number, number][]
  duration_ms: number
}

export interface SyncHistoryEntry {
  device_id: string
  device_name: string | null
  synced_at: string
  rows_sent: number
  rows_received: number
  direction: string
}

// ---- Challenge ----

export type ChallengeTargetType = 'evidence' | 'skill_proof' | 'opinion'
export type ChallengeStatus = 'pending' | 'reviewing' | 'upheld' | 'rejected' | 'expired'

export interface EvidenceChallenge {
  id: string
  challenger: string
  target_type: string
  target_ids: string[]
  evidence_cids: string[]
  reason: string
  stake_lovelace: number
  stake_tx_hash: string | null
  status: string
  dao_id: string
  learner_address: string
  reviewed_by: string[]
  resolution_tx: string | null
  signature: string
  created_at: string
  resolved_at: string | null
  expires_at: string | null
}

export interface ChallengeVote {
  id: string
  challenge_id: string
  voter: string
  upheld: boolean
  reason: string | null
  voted_at: string
}

export interface SubmitChallengeParams {
  target_type: string
  target_ids: string[]
  evidence_cids: string[]
  reason: string
  stake_lovelace: number
  dao_id: string
  learner_address: string
}

export interface ChallengeResolution {
  challenge_id: string
  status: string
  votes_upheld: number
  votes_rejected: number
  proofs_invalidated: number
  reputation_zeroed: boolean
}

// ---- Attestation ----

export type AttestorRole = 'assessor' | 'proctor'
export type AttestationType = 'co_sign' | 'proctor_verify' | 'skill_verify'

export interface AttestationRequirement {
  skill_id: string
  proficiency_level: string
  required_attestors: number
  dao_id: string
  set_by_proposal: string | null
  created_at: string
  updated_at: string
}

export interface EvidenceAttestation {
  id: string
  evidence_id: string
  attestor_address: string
  attestor_role: string
  attestation_type: string
  integrity_score: number | null
  session_cid: string | null
  signature: string
  created_at: string
}

export interface AttestationStatus {
  evidence_id: string
  skill_id: string
  proficiency_level: string
  required_attestors: number
  current_attestors: number
  is_fully_attested: boolean
  attestations: EvidenceAttestation[]
}

export interface SubmitAttestationParams {
  evidence_id: string
  attestation_type?: string | null
  integrity_score?: number | null
  session_cid?: string | null
}

// ---- Taxonomy (skill graph) ----

export interface SubjectFieldInfo {
  id: string
  name: string
  description: string | null
  icon_emoji: string | null
  subject_count: number
  skill_count: number
  created_at: string | null
}

export interface SubjectInfo {
  id: string
  name: string
  description: string | null
  subject_field_id: string | null
  subject_field_name: string | null
  skill_count: number
  created_at: string | null
}

export interface SkillInfo {
  id: string
  name: string
  description: string | null
  subject_id: string | null
  subject_name: string | null
  subject_field_id: string | null
  subject_field_name: string | null
  bloom_level: string
  prerequisite_count: number
  dependent_count: number
  created_at: string | null
}

export interface SkillSummary {
  id: string
  name: string
  bloom_level: string
  subject_name: string | null
}

export interface SkillRelation {
  skill_id: string
  skill_name: string
  bloom_level: string
  relation_type: string
}

export interface SkillDetail {
  skill: SkillInfo
  prerequisites: SkillSummary[]
  dependents: SkillSummary[]
  related: SkillRelation[]
}

export interface SkillGraphEdge {
  skill_id: string
  skill_name: string
  skill_bloom: string
  prerequisite_id: string
  prerequisite_name: string
  prerequisite_bloom: string
}

export interface ElementSkillTag {
  skill_id: string
  skill_name: string
  bloom_level: string
  weight: number
}

// ---- Integrity / Sentinel ----

export type SessionOutcome = 'clean' | 'flagged' | 'suspended'
export type FlagSeverity = 'info' | 'warning' | 'critical'
export type FlagType =
  | 'multi_account'
  | 'low_integrity'
  | 'speed_anomaly'
  | 'device_change'
  | 'tab_switching'
  | 'paste_detected'
  | 'no_face'
  | 'multiple_faces'
  | 'behavior_shift'
  | 'face_mismatch'
  | 'prolonged_absence'
  | 'frequent_absence'
  | 'bot_suspected'
  | 'devtools_detected'

export interface IntegritySession {
  id: string
  enrollment_id: string
  status: string
  integrity_score: number | null
  started_at: string
  ended_at: string | null
}

export interface IntegritySnapshot {
  id: string
  session_id: string
  typing_score: number | null
  mouse_score: number | null
  human_score: number | null
  tab_score: number | null
  paste_score: number | null
  devtools_score: number | null
  camera_score: number | null
  composite_score: number | null
  captured_at: string
}

export interface SignalData {
  typing_consistency: number
  typing_speed_wpm: number
  mouse_consistency: number
  is_human_likely: boolean
  face_present?: boolean
  face_count?: number
  face_consistency?: number
  tab_switches: number
  unfocused_ms: number
  devtools_detected: boolean
  paste_events: number
  pasted_chars: number
  environment_changed: boolean
  ai_keystroke_anomaly?: number
  ai_mouse_human_prob?: number
  ai_face_similarity?: number
  ai_face_match?: boolean
}

export interface BehavioralProfile {
  userId: string
  deviceFingerprint: string
  typingPattern: {
    avgDwellTime: number
    avgFlightTime: number
    speedWpm: number
    sampleCount: number
  }
  mousePattern: {
    avgVelocity: number
    avgAcceleration: number
    clickPrecision: number
    sampleCount: number
  }
  lastUpdated: number
  aiModels?: {
    keystrokeAutoencoder?: Record<string, unknown>
    mouseCNN?: Record<string, unknown>
    faceEnrollment?: {
      vector: number[]
      frameCount: number
      updatedAt: number
    }
  }
}

export interface StartSessionResponse {
  session_id: string
}

export interface SubmitSnapshotRequest {
  session_id: string
  element_id: string
  integrity_score: number
  consistency_score: number
  typing_score: number | null
  mouse_score: number | null
  human_score: number | null
  tab_score: number | null
  paste_score: number | null
  devtools_score: number | null
  camera_score: number | null
  anomaly_flags: string[]
}

export interface EndSessionRequest {
  overall_integrity_score: number
  overall_consistency_score: number
}

/** Content document structure returned from IPFS */
export interface ContentDocument {
  content_type: string
  body: string
  metadata?: Record<string, unknown>
}

/** Quiz question model embedded in content */
export interface QuizQuestion {
  id: string
  type: 'single_choice' | 'multiple_choice' | 'true_false' | 'short_answer'
  prompt: string
  options?: string[]
  correct_indices?: number[]
  correct_answer?: string
  explanation?: string
  points: number
  difficulty: number
}

/** A complete quiz/assessment definition */
export interface QuizDefinition {
  title: string
  description?: string
  time_limit_seconds?: number
  pass_threshold: number
  questions: QuizQuestion[]
  skill_tags?: { skill_id: string; weight: number }[]
}

/** Result of completing a quiz */
export interface QuizResult {
  total_points: number
  earned_points: number
  score: number
  passed: boolean
  answers: { question_id: string; correct: boolean; points: number }[]
  time_spent_seconds: number
}

// ---- P2P ----

export interface P2PStatus {
  is_running: boolean
  peer_id: string | null
  listening_addresses: string[]
  connected_peers: number
  subscribed_topics: string[]
  nat_status: string
  relay_addresses: string[]
}

/// Connected peer info returned by `p2p_peers`.
///
/// Currently the backend returns just the peer ID string for each
/// connected peer. Address and protocol info may be added later when
/// the swarm command channel supports richer queries.
export type PeerInfo = string

// ---- Tutoring ----

export interface TutoringSessionInfo {
  id: string
  title: string
  ticket: string | null
  status: 'active' | 'ended' | 'cancelled'
  created_at: string
  ended_at: string | null
}

export interface TutoringPeer {
  node_id: string
  display_name: string | null
  broadcasts: string[]
  connected: boolean
}

export interface TutoringSessionStatus {
  session_id: string
  session_title: string
  ticket: string
  peers: TutoringPeer[]
  video_enabled: boolean
  audio_enabled: boolean
  screen_sharing: boolean
  started_at: number
}

export interface TutoringVideoFrame {
  node_id: string
  jpeg_b64: string
  width: number
  height: number
}

export interface TutoringChatMessage {
  sender: string
  sender_name: string | null
  text: string
  timestamp: number
}

export interface DeviceCheckResult {
  has_camera: boolean
  camera_name: string | null
  has_audio: boolean
  error: string | null
}

export interface AudioDeviceInfo {
  id: string
  name: string | null
  is_default: boolean
}

export interface CameraDeviceInfo {
  index: string
  name: string
}

export interface DeviceList {
  audio_inputs: AudioDeviceInfo[]
  audio_outputs: AudioDeviceInfo[]
  cameras: CameraDeviceInfo[]
  selected_audio_input: string | null
  selected_audio_output: string | null
}

export interface AudioLevelEvent {
  mic_level: number
  output_level: number
}

// ---- Health ----

export interface HealthResponse {
  status: string
  version: string
  database: string
}

// ---- Classrooms ----

export interface Classroom {
  id: string
  name: string
  description: string | null
  icon_emoji: string | null
  owner_address: string
  invite_code: string | null
  status: 'active' | 'archived'
  created_at: string
  updated_at: string
  member_count: number | null
  my_role: 'owner' | 'moderator' | 'member' | null
}

export interface ClassroomMember {
  classroom_id: string
  stake_address: string
  role: 'owner' | 'moderator' | 'member'
  display_name: string | null
  joined_at: string
}

export interface ClassroomChannel {
  id: string
  classroom_id: string
  name: string
  description: string | null
  channel_type: 'text' | 'announcement'
  position: number
  created_at: string
}

export interface ClassroomMessage {
  id: string
  channel_id: string
  classroom_id: string
  sender_address: string
  sender_name: string | null
  content: string
  deleted: boolean
  edited_at: string | null
  sent_at: string
  received_at: string
}

export interface JoinRequest {
  id: string
  classroom_id: string
  stake_address: string
  display_name: string | null
  message: string | null
  status: 'pending' | 'approved' | 'denied'
  reviewed_by: string | null
  requested_at: string
  reviewed_at: string | null
}

export interface ClassroomCall {
  id: string
  classroom_id: string
  channel_id: string | null
  title: string
  ticket: string | null
  started_by: string
  status: 'active' | 'ended'
  started_at: string
  ended_at: string | null
}

// Tauri event payloads
export interface ClassroomMessageEvent {
  classroom_id: string
  channel_id: string
  message: {
    id: string
    channel_id: string
    classroom_id: string
    sender_address: string
    sender_name: string | null
    content: string
    sent_at: string
  }
}

export interface ClassroomMetaEvent {
  classroom_id: string
  event_type:
    | 'JoinRequest'
    | 'MemberApproved'
    | 'MemberDenied'
    | 'MemberLeft'
    | 'MemberKicked'
    | 'RoleChanged'
    | 'CallStarted'
    | 'CallEnded'
  data: Record<string, unknown>
}
