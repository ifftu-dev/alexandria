import { useLocalApi } from '@/composables/useLocalApi'

/** One person on an organisation's interview panel. */
export interface PanelMember {
  id: string
  name: string
}

/** A skill the candidate has shared with the organisation, shown to the panel. */
export interface PanelEvidence {
  skillId: string
  name: string
  bloomLevel: number
  level: number
  issuerClusters: number
}

/** An organisation's interview as Alexandria Cloud shows it to one panelist. */
export interface PanelInterview {
  interviewId: string
  organization: string
  orgDomain: string | null
  roleLabel: string
  status: string
  chosenSlot: number | null
  candidate: { did: string; displayName: string | null }
  criteria: string[]
  me: PanelMember
  panel: PanelMember[]
  room: { ticket: string | null; openedAt: string | null }
  candidateEvidence: PanelEvidence[]
  /** Present when Cloud knows about the integrity check (always, for a current Cloud). */
  integrity?: PanelIntegrity | null
}

/** One run of the candidate's integrity check, as their app reported it. */
export interface PanelIntegrityResult {
  outcome: 'clear' | 'flagged'
  score: number | null
  startedAt: string | null
  endedAt: string | null
  snapshots: number
  camera: boolean
  cameraCoverage: number
  flags: { flag: string; severity: 'info' | 'warning' | 'critical'; count: number }[]
  receivedAt: string
}

export interface PanelIntegrity {
  requested: boolean
  results: PanelIntegrityResult[]
}

export interface PanelSession {
  sessionId: string
  interview: PanelInterview
}

export type PanelRecommendation = 'strong_yes' | 'yes' | 'no' | 'strong_no'

export interface PanelRating {
  criterion: string
  rating: number | null
}

/** Where an interview held in Alexandria can be joined, once it is open. */
export interface CandidateRoom {
  ticket: string | null
  openedAt: string | null
  /** The organisation asked the candidate to run the integrity check. */
  integrityCheck?: boolean
}

export function usePanel() {
  const { invoke } = useLocalApi()

  /** Open a panelist's join link; returns the local interview session it attaches to. */
  function openLink(link: string) {
    return invoke<PanelSession>('interview_panel_open', { link })
  }

  /** The panel interview behind a local session. Rejects for an interview that is not a panel seat. */
  function status(sessionId: string) {
    return invoke<PanelInterview>('interview_panel_status', { sessionId })
  }

  function setRoom(sessionId: string, ticket: string) {
    return invoke<PanelInterview>('interview_panel_set_room', { sessionId, ticket })
  }

  function sendScorecard(
    sessionId: string,
    ratings: PanelRating[],
    recommendation: PanelRecommendation,
    notes: string,
  ) {
    return invoke<unknown>('interview_panel_scorecard', { sessionId, ratings, recommendation, notes })
  }

  /** For the candidate: the room of an accepted interview held in Alexandria. */
  function candidateRoom(directoryUrl: string, interviewId: string) {
    return invoke<CandidateRoom>('hiring_interview_room', { directoryUrl, interviewId })
  }

  return { openLink, status, setRoom, sendScorecard, candidateRoom }
}
