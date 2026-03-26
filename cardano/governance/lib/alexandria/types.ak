//// Alexandria Governance — Core Type Definitions
////
//// On-chain types for the Alexandria DAO governance system.
//// Reputation tokens use CIP-68 (two-token pattern):
////   (100) reference NFT — locked at soulbound script, holds reputation datum
////   (222) wallet receipt — in user's wallet, visible but carries no governance weight
////
//// Asset name encoding for reputation tokens:
////   asset_name = subject_id (16 bytes, UUID) ++ role_byte (1 byte: 0=instructor, 1=learner)
////
//// Governance validators read reputation via CIP-31 reference inputs.

use aiken/crypto.{ScriptHash, VerificationKeyHash}
use cardano/assets.{AssetName, PolicyId}
use cardano/transaction.{OutputReference}

// ─── CIP-68 label prefixes ──────────────────────────────────────────────────

/// CIP-68 reference NFT label prefix (100 in big-endian, 4 bytes)
pub const cip68_reference_label = #"000643b0"

/// CIP-68 user token label prefix (222 in big-endian, 4 bytes)
pub const cip68_user_token_label = #"000de140"

// ─── Reputation roles ───────────────────────────────────────────────────────

/// Roles for reputation scoping (mirrors api/internal/reputation/models.go).
pub type ReputationRole {
  Instructor
  Learner
}

pub fn role_to_int(role: ReputationRole) -> Int {
  when role is {
    Instructor -> 0
    Learner -> 1
  }
}

pub fn int_to_role(n: Int) -> ReputationRole {
  when n is {
    0 -> Instructor
    1 -> Learner
    _ -> fail @"invalid role byte"
  }
}

// ─── Proficiency levels (Bloom's taxonomy) ──────────────────────────────────

pub type ProficiencyLevel {
  Remember
  Understand
  Apply
  Analyze
  Evaluate
  Create
}

pub fn proficiency_to_int(level: ProficiencyLevel) -> Int {
  when level is {
    Remember -> 0
    Understand -> 1
    Apply -> 2
    Analyze -> 3
    Evaluate -> 4
    Create -> 5
  }
}

pub fn int_to_proficiency(n: Int) -> ProficiencyLevel {
  when n is {
    0 -> Remember
    1 -> Understand
    2 -> Apply
    3 -> Analyze
    4 -> Evaluate
    5 -> Create
    _ -> fail @"invalid proficiency byte"
  }
}

/// True if actual >= required in Bloom's hierarchy.
pub fn proficiency_met(actual: ProficiencyLevel, required: ProficiencyLevel) -> Bool {
  proficiency_to_int(actual) >= proficiency_to_int(required)
}

// ─── Reputation datum (CIP-68 extra field) ──────────────────────────────────

/// A single skill-level reputation score within the aggregated datum.
pub type SkillScore {
  /// Skill identifier (16-byte UUID, same as in the API DB)
  skill_id: ByteArray,
  /// Bloom's proficiency level
  proficiency: ProficiencyLevel,
  /// Impact score scaled by 10^6 (avoids floats on-chain)
  impact_score: Int,
  /// Confidence scaled by 10^4
  confidence: Int,
  /// Number of evidence records backing this score
  evidence_count: Int,
}

/// The reputation datum stored in the CIP-68 reference NFT's inline datum.
/// One token per (subject, role), with skill-level detail in the skills list.
pub type ReputationDatum {
  /// Owner's verification key hash (the user whose reputation this is)
  owner: VerificationKeyHash,
  /// Subject identifier (16-byte UUID matching the asset name prefix)
  subject_id: ByteArray,
  /// Role (instructor or learner)
  role: ReputationRole,
  /// Per-skill reputation scores
  skills: List<SkillScore>,
  /// Computation spec version (e.g. 1 for "v1")
  computation_spec: Int,
  /// Time window start (POSIX milliseconds)
  window_start: Int,
  /// Time window end (POSIX milliseconds)
  window_end: Int,
}

// ─── Soulbound spending script ──────────────────────────────────────────────

/// Redeemer for the soulbound spending script that locks reputation tokens.
pub type SoulboundRedeemer {
  /// Update the datum (reputation changed). Output must return to same
  /// script address with same owner. Requires authorized_minter OR owner.
  UpdateReputation
  /// Burn/revoke the token. Requires authorized_minter signature.
  RevokeReputation
}

// ─── Reputation minting policy (CIP-68) ─────────────────────────────────────

/// Redeemer for the CIP-68 reputation minting policy.
pub type ReputationMintRedeemer {
  /// Mint a new (100) reference + (222) wallet receipt pair
  MintReputation
  /// Burn both tokens (on revocation)
  BurnReputation
}

// ─── DAO scope ──────────────────────────────────────────────────────────────

/// What part of the knowledge taxonomy a DAO governs.
pub type DaoScope {
  /// Top-level DAO for a Subject Field (whitepaper §4.2.1)
  SubjectFieldDao { scope_id: ByteArray }
  /// Sub-DAO for a single Subject (whitepaper §4.2.2)
  SubjectDao { scope_id: ByteArray }
}

// ─── DAO registry ───────────────────────────────────────────────────────────

/// Datum for a DAO's state UTxO at the registry validator.
pub type DaoDatum {
  /// Taxonomy scope
  scope: DaoScope,
  /// Policy ID of the reputation minting policy — governance reads these tokens
  reputation_policy: PolicyId,
  /// Subject IDs that qualify for membership (reputation tokens with these
  /// subject_ids in their asset name are eligible)
  membership_subjects: List<ByteArray>,
  /// Minimum proficiency for basic DAO membership
  min_membership_proficiency: ProficiencyLevel,
  /// Policy ID of this DAO's state token
  state_token_policy: PolicyId,
  /// Current committee members
  committee: List<VerificationKeyHash>,
  /// Number of committee seats
  committee_size: Int,
  /// Election interval in POSIX milliseconds
  election_interval: Int,
  /// Current term start (POSIX ms)
  term_start: Int,
  /// Current term end (POSIX ms)
  term_end: Int,
}

/// Actions on DAO registry UTxOs.
pub type DaoRedeemer {
  /// Create a new DAO (mints state token)
  CreateDao
  /// Update DAO parameters (requires committee quorum)
  UpdateDao
  /// Install new committee after election finalization
  InstallCommittee { election_ref: OutputReference }
}

// ─── Elections ──────────────────────────────────────────────────────────────

pub type ElectionPhase {
  Nomination
  Voting
  Finalized
}

pub type Nominee {
  /// Nominee's verification key hash
  key_hash: VerificationKeyHash,
  /// Whether the nominee has accepted
  accepted: Bool,
}

/// Datum for an election UTxO.
pub type ElectionDatum {
  /// DAO this election belongs to (state token reference)
  dao_state_token_policy: PolicyId,
  dao_state_token_name: AssetName,
  /// Monotonically increasing election ID per DAO
  election_id: Int,
  /// Current phase
  phase: ElectionPhase,
  /// Number of seats
  seats: Int,
  /// Minimum proficiency to be nominated (higher than voter threshold)
  nominee_min_proficiency: ProficiencyLevel,
  /// Minimum proficiency to vote
  voter_min_proficiency: ProficiencyLevel,
  /// Subject IDs for voter/nominee eligibility
  membership_subjects: List<ByteArray>,
  /// Reputation policy ID (for verifying reference input tokens)
  reputation_policy: PolicyId,
  /// Nominees list
  nominees: List<Nominee>,
  /// POSIX ms when nomination ends
  nomination_end: Int,
  /// POSIX ms when voting ends
  voting_end: Int,
  /// Vote receipt minting policy
  vote_receipt_policy: PolicyId,
}

pub type ElectionRedeemer {
  /// Committee opens a new election
  OpenElection
  /// A nominee accepts their nomination
  AcceptNomination { nominee_index: Int }
  /// Transition from nomination to voting phase
  StartVoting
  /// Finalize the election after voting ends
  FinalizeElection { winners: List<VerificationKeyHash> }
}

// ─── Proposals ──────────────────────────────────────────────────────────────

pub type ProposalStatus {
  Draft
  Published
  Approved
  Rejected
  Cancelled
}

pub type ProposalCategory {
  CurriculumChange
  AssessmentStandards
  PolicyChange
  TreasuryAllocation { recipient: VerificationKeyHash, amount_lovelace: Int }
  GeneralMotion
}

/// Datum for a proposal UTxO.
pub type ProposalDatum {
  /// DAO reference
  dao_state_token_policy: PolicyId,
  dao_state_token_name: AssetName,
  /// Monotonically increasing proposal ID per DAO
  proposal_id: Int,
  /// Author
  author: VerificationKeyHash,
  /// Current status
  status: ProposalStatus,
  /// Category
  category: ProposalCategory,
  /// Minimum proficiency to vote (whitepaper §4.5.1)
  min_vote_proficiency: ProficiencyLevel,
  /// Subject IDs for voter eligibility
  membership_subjects: List<ByteArray>,
  /// Reputation policy for verification
  reputation_policy: PolicyId,
  /// IPFS CID of full proposal content
  content_cid: ByteArray,
  /// POSIX ms when voting ends (set when Published)
  voting_deadline: Int,
  /// Vote tallies
  votes_for: Int,
  votes_against: Int,
  /// Vote receipt policy
  vote_receipt_policy: PolicyId,
}

pub type ProposalRedeemer {
  /// Any DAO member submits a draft
  SubmitDraft
  /// Committee approves draft for voting
  ApproveForVoting { voting_duration: Int }
  /// Author or committee cancels
  CancelProposal
  /// Cast a vote on a Published proposal
  CastProposalVote { vote_for: Bool }
  /// Resolve after voting deadline
  ResolveProposal
}

// ─── Vote receipts ──────────────────────────────────────────────────────────

pub type VoteTargetType {
  ElectionVote
  ProposalVote
}

pub type VoteReceiptRedeemer {
  /// Mint when casting a vote
  MintReceipt
  /// Burn after finalization
  BurnReceipt
}
