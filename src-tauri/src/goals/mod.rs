//! Learner goals → ideal skill graph.
//!
//! Exams / K-12 curricula / job roles resolve to target skills via the
//! bundled [`goal_templates`](crate::db::schema) that ship with the app;
//! free-text job
//! descriptions are matched on-device against the taxonomy by [`jd_parser`].
//! Both paths produce a set of target skill IDs consumed by the existing
//! learning-path pipeline (`commands::graph::compute_path`).

pub mod jd_parser;
