//! GossipSub peer scoring configuration for the Alexandria P2P network.
//!
//! Per spec §7.3: "Peers that repeatedly send invalid messages are scored
//! down by GossipSub's peer scoring mechanism, eventually disconnected."
//!
//! Scoring parameters are tuned for a learning platform with:
//! - Low message frequency (not real-time chat)
//! - High value per message (credentials, evidence, taxonomy)
//! - Strong penalties for invalid/unauthorized messages on taxonomy topic
//! - Moderate rewards for first-hop message delivery

use std::collections::HashMap;
use std::time::Duration;

use libp2p::gossipsub::{IdentTopic, PeerScoreParams, PeerScoreThresholds, TopicScoreParams};

use super::types::{
    TOPIC_CATALOG, TOPIC_EVIDENCE, TOPIC_GOVERNANCE, TOPIC_OPINIONS, TOPIC_PROFILES, TOPIC_TAXONOMY,
};

/// Build Alexandria-specific GossipSub peer score parameters.
///
/// Configures per-topic scoring for all five gossip topics with
/// parameters tuned for the Alexandria network's message patterns.
pub fn build_peer_score_params() -> PeerScoreParams {
    let mut topics = HashMap::new();

    // Insert per-topic params for each gossip topic
    topics.insert(
        IdentTopic::new(TOPIC_CATALOG).hash(),
        topic_params_catalog(),
    );
    topics.insert(
        IdentTopic::new(TOPIC_EVIDENCE).hash(),
        topic_params_evidence(),
    );
    topics.insert(
        IdentTopic::new(TOPIC_TAXONOMY).hash(),
        topic_params_taxonomy(),
    );
    topics.insert(
        IdentTopic::new(TOPIC_GOVERNANCE).hash(),
        topic_params_governance(),
    );
    topics.insert(
        IdentTopic::new(TOPIC_PROFILES).hash(),
        topic_params_profiles(),
    );
    topics.insert(
        IdentTopic::new(TOPIC_OPINIONS).hash(),
        topic_params_opinions(),
    );

    PeerScoreParams {
        topics,
        // Cap positive topic contribution to prevent score inflation
        topic_score_cap: 100.0,
        // P5: Application-specific weight — used for Alexandria reputation
        // integration (set via set_application_score in future PRs)
        app_specific_weight: 1.0,
        // P6: IP colocation penalty — discourage Sybil attacks from same IP
        ip_colocation_factor_weight: -10.0,
        ip_colocation_factor_threshold: 3.0,
        // P7: Protocol misbehaviour penalty
        behaviour_penalty_weight: -10.0,
        behaviour_penalty_threshold: 0.0,
        behaviour_penalty_decay: 0.9,
        // Decay configuration
        decay_interval: Duration::from_secs(1),
        decay_to_zero: 0.01,
        retain_score: Duration::from_secs(3600), // Remember scores for 1 hour
        // Slow peer penalties
        slow_peer_weight: -0.2,
        slow_peer_threshold: 0.0,
        slow_peer_decay: 0.9,
        ..Default::default()
    }
}

/// Build GossipSub peer score thresholds.
///
/// These thresholds determine when peers start losing functionality:
/// - Below gossip_threshold: gossip suppressed
/// - Below publish_threshold: publishing suppressed
/// - Below graylist_threshold: messages dropped entirely
pub fn build_peer_score_thresholds() -> PeerScoreThresholds {
    PeerScoreThresholds {
        // Below -10: stop sending gossip control messages to this peer
        gossip_threshold: -10.0,
        // Below -50: stop publishing to this peer
        publish_threshold: -50.0,
        // Below -80: drop all messages from this peer (graylist)
        graylist_threshold: -80.0,
        // Peers need score >= 5 for peer exchange to be trusted
        accept_px_threshold: 5.0,
        // Trigger opportunistic grafting when median mesh score > 3
        opportunistic_graft_threshold: 3.0,
    }
}

/// Catalog topic scoring — course announcements.
///
/// Medium frequency, moderate value. Rewards first deliveries
/// to incentivize being a good relay for course discovery.
fn topic_params_catalog() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.5,
        // P1: Time in mesh — reward stable mesh membership
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,
        // P2: First message deliveries — reward being first to relay
        first_message_deliveries_weight: 2.0,
        first_message_deliveries_decay: 0.9,
        first_message_deliveries_cap: 50.0,
        // P3: Mesh message deliveries — penalize freeloaders
        mesh_message_deliveries_weight: -0.5,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 20.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(30),
        // P3b: Mesh failure penalty
        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.9,
        // P4: Invalid message penalty — moderate for catalog
        invalid_message_deliveries_weight: -10.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Evidence topic scoring — skill evidence broadcasts.
///
/// Lower frequency than catalog. Evidence is high-value
/// (feeds into reputation computation).
fn topic_params_evidence() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.7,
        // P1: Time in mesh
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,
        // P2: First message deliveries — evidence relay is valuable
        first_message_deliveries_weight: 3.0,
        first_message_deliveries_decay: 0.9,
        first_message_deliveries_cap: 30.0,
        // P3: Mesh message deliveries
        mesh_message_deliveries_weight: -0.5,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 15.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(30),
        // P3b
        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.9,
        // P4: Invalid evidence is a stronger penalty
        invalid_message_deliveries_weight: -15.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Taxonomy topic scoring — DAO-ratified skill graph updates.
///
/// Low frequency, very high value. Only DAO committee members should
/// publish. Strongest invalid message penalty of all topics.
fn topic_params_taxonomy() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 1.0,
        // P1: Time in mesh
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,
        // P2: First deliveries — taxonomy updates are rare and important
        first_message_deliveries_weight: 5.0,
        first_message_deliveries_decay: 0.95,
        first_message_deliveries_cap: 10.0,
        // P3: Mesh deliveries — relaxed (taxonomy is infrequent)
        mesh_message_deliveries_weight: -0.1,
        mesh_message_deliveries_decay: 0.95,
        mesh_message_deliveries_cap: 5.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(60),
        // P3b
        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.95,
        // P4: STRONGEST invalid message penalty — unauthorized taxonomy
        // updates are a serious attack vector
        invalid_message_deliveries_weight: -50.0,
        invalid_message_deliveries_decay: 0.3,
    }
}

/// Governance topic scoring — DAO proposals, votes, committee changes.
///
/// Low-to-medium frequency. Committee changes are critical.
fn topic_params_governance() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.8,
        // P1: Time in mesh
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,
        // P2: First deliveries
        first_message_deliveries_weight: 3.0,
        first_message_deliveries_decay: 0.9,
        first_message_deliveries_cap: 20.0,
        // P3: Mesh deliveries
        mesh_message_deliveries_weight: -0.3,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 10.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(30),
        // P3b
        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.9,
        // P4: Strong penalty — governance manipulation is dangerous
        invalid_message_deliveries_weight: -30.0,
        invalid_message_deliveries_decay: 0.3,
    }
}

/// Profiles topic scoring — user profile announcements.
///
/// Medium frequency, low value. Most permissive scoring.
fn topic_params_profiles() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.3,
        // P1: Time in mesh
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,
        // P2: First deliveries — low reward for profile relay
        first_message_deliveries_weight: 1.0,
        first_message_deliveries_decay: 0.9,
        first_message_deliveries_cap: 50.0,
        // P3: Mesh deliveries — lenient
        mesh_message_deliveries_weight: -0.1,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 20.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(30),
        // P3b
        mesh_failure_penalty_weight: -0.1,
        mesh_failure_penalty_decay: 0.9,
        // P4: Moderate penalty for invalid profiles
        invalid_message_deliveries_weight: -5.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

/// Opinions topic scoring — Field Commentary videos.
///
/// Tighter than catalog: opinions are cheaper to produce (no course
/// structure, no chapters) and therefore a more attractive spam
/// target. Higher invalid-message weight reflects the fact that an
/// invalid opinion (failed credential check, bad signature) is a
/// clear protocol violation.
fn topic_params_opinions() -> TopicScoreParams {
    TopicScoreParams {
        topic_weight: 0.4,
        // P1: time in mesh — same as catalog
        time_in_mesh_weight: 0.5,
        time_in_mesh_quantum: Duration::from_secs(1),
        time_in_mesh_cap: 100.0,
        // P2: first-delivery rewards — modest
        first_message_deliveries_weight: 1.5,
        first_message_deliveries_decay: 0.9,
        first_message_deliveries_cap: 30.0,
        // P3: mesh deliveries
        mesh_message_deliveries_weight: -0.5,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 15.0,
        mesh_message_deliveries_threshold: 1.0,
        mesh_message_deliveries_window: Duration::from_millis(500),
        mesh_message_deliveries_activation: Duration::from_secs(30),
        // P3b
        mesh_failure_penalty_weight: -0.5,
        mesh_failure_penalty_decay: 0.9,
        // P4: stronger than catalog — invalid opinions (bad credentials,
        // unknown subject_field, or bad signature) are a clear attack.
        invalid_message_deliveries_weight: -20.0,
        invalid_message_deliveries_decay: 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_score_params_are_valid() {
        let params = build_peer_score_params();
        assert!(params.validate().is_ok(), "peer score params must validate");
    }

    #[test]
    fn peer_score_thresholds_are_valid() {
        let thresholds = build_peer_score_thresholds();
        assert!(
            thresholds.validate().is_ok(),
            "peer score thresholds must validate"
        );
    }

    #[test]
    fn all_scored_topics_have_params() {
        let params = build_peer_score_params();
        assert_eq!(
            params.topics.len(),
            6,
            "should have params for all 6 scored topics (5 originals + opinions)"
        );
    }

    #[test]
    fn taxonomy_has_strongest_invalid_penalty() {
        let params = build_peer_score_params();
        let taxonomy_hash = IdentTopic::new(TOPIC_TAXONOMY).hash();
        let taxonomy_params = params.topics.get(&taxonomy_hash).unwrap();

        // Taxonomy should have the strongest P4 penalty (most negative)
        for (hash, topic_params) in &params.topics {
            if *hash != taxonomy_hash {
                assert!(
                    taxonomy_params.invalid_message_deliveries_weight
                        <= topic_params.invalid_message_deliveries_weight,
                    "taxonomy P4 should be <= all other topics"
                );
            }
        }
    }

    #[test]
    fn thresholds_are_properly_ordered() {
        let t = build_peer_score_thresholds();
        // graylist <= publish <= gossip <= 0
        assert!(t.graylist_threshold <= t.publish_threshold);
        assert!(t.publish_threshold <= t.gossip_threshold);
        assert!(t.gossip_threshold <= 0.0);
        // positive thresholds
        assert!(t.accept_px_threshold >= 0.0);
        assert!(t.opportunistic_graft_threshold >= 0.0);
    }

    #[test]
    fn each_topic_params_validates_individually() {
        assert!(topic_params_catalog().validate().is_ok());
        assert!(topic_params_evidence().validate().is_ok());
        assert!(topic_params_taxonomy().validate().is_ok());
        assert!(topic_params_governance().validate().is_ok());
        assert!(topic_params_profiles().validate().is_ok());
    }
}
