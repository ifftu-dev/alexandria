//// Alexandria Anchor Sponsor — On-chain types
////
//// The treasury locks ADA at the `anchor_sponsor` spending validator so
//// any node can pay the fee of a credential-anchor transaction without a
//// shipped private key. The validator bounds each spend (fee cap) and
//// rate-limits each sponsor UTxO (window); it never inspects metadata.
////
//// Plutus Data encoding (what off-chain builders must produce):
////   SponsorDatum { next_allowed }  = Constr 0 [Int]   (POSIX ms)
////   Sponsor                        = Constr 0 []
////   Reclaim                        = Constr 1 []

/// Inline datum on every sponsor UTxO.
pub type SponsorDatum {
  /// Earliest POSIX time (ms) at which the validity-interval lower bound
  /// of the next `Sponsor` spend may start.
  next_allowed: Int,
}

pub type SponsorRedeemer {
  /// Pay one anchor tx fee (at most `fee_cap`) and re-lock the rest.
  Sponsor
  /// m-of-n reclaim multisig: withdraw, re-split or migrate the pool.
  Reclaim
}
