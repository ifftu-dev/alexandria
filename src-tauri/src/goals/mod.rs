//! Learner goals → ideal skill graph.
//!
//! Exams / K-12 curricula / job roles resolve to target skills via curated,
//! DAO-ratified [`goal_templates`](crate::db::schema); free-text job
//! descriptions are matched on-device against the taxonomy by [`jd_parser`].
//! Both paths produce a set of target skill IDs consumed by the existing
//! learning-path pipeline (`commands::graph::compute_path`).

pub mod jd_parser;
