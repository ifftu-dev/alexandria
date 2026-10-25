//! Bundled built-in data installed into every profile database.
//!
//! The skill taxonomy, skill synonyms, goal templates and question banks ship
//! with the app as labelled built-in content. No committee ratified them:
//! `taxonomy_version = 'bundled'` identifies the goal templates and question
//! banks, and `ratified = 1` only marks those rows usable until the baseline
//! schema replaces that column. No personas, credentials, opinions, courses,
//! classrooms or governance rows are installed; the demo course corpus lives
//! in `demo-world/content/` as non-authoritative source material. Profile
//! activation separately imports owned example resources after identity
//! initialization (see `commands::demo_courses`).
//!
//! Installation is one transaction and idempotent, and runs on every profile
//! open: taxonomy rows are upserted by id, so a profile created by an older
//! build gains skills added since; goal templates are replaced by id; the
//! rest use `INSERT OR IGNORE` and keyed updates.

use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::{params, Connection};

use super::Database;
use serde::Deserialize;

pub(crate) fn bundled_snapshot() -> Result<MutexGuard<'static, Database>, String> {
    static SNAPSHOT: OnceLock<Result<Mutex<Database>, String>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| {
            let db = Database::open_in_memory().map_err(|e| e.to_string())?;
            db.run_migrations().map_err(|e| e.to_string())?;
            install_bundled_data(db.conn())?;
            Ok(Mutex::new(db))
        })
        .as_ref()
        .map_err(Clone::clone)?
        .lock()
        .map_err(|e| e.to_string())
}

const PUBLIC_TAXONOMY_JSON: &str = include_str!("../../../bootstrap/public_taxonomy.json");

#[derive(Debug, Deserialize)]
struct BootstrapTaxonomyPayload {
    subject_fields: Vec<BootstrapSubjectField>,
    subjects: Vec<BootstrapSubject>,
    skills: Vec<BootstrapSkill>,
    skill_prerequisites: Vec<BootstrapSkillPrerequisite>,
    skill_relations: Vec<BootstrapSkillRelation>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSubjectField {
    id: String,
    name: String,
    description: Option<String>,
    icon_emoji: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSubject {
    id: String,
    name: String,
    description: Option<String>,
    subject_field_id: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkill {
    id: String,
    name: String,
    description: Option<String>,
    subject_id: String,
    bloom_level: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkillPrerequisite {
    skill_id: String,
    prerequisite_id: String,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkillRelation {
    skill_id: String,
    related_skill_id: String,
    relation_type: String,
}

/// The subjects the bundled skill taxonomy covers, grouped by field, as one
/// line of text: "Field (Subject, Subject); Field (Subject)". The skills table
/// is only ever filled from this taxonomy, so this is the whole catalog.
pub(crate) fn subject_catalog() -> &'static str {
    static CATALOG: OnceLock<String> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let Ok(payload) = serde_json::from_str::<BootstrapTaxonomyPayload>(PUBLIC_TAXONOMY_JSON)
        else {
            return String::new();
        };
        let mut fields: Vec<(&str, Vec<&str>)> = payload
            .subject_fields
            .iter()
            .map(|f| {
                let mut subjects: Vec<&str> = payload
                    .subjects
                    .iter()
                    .filter(|s| s.subject_field_id == f.id)
                    .map(|s| s.name.as_str())
                    .collect();
                subjects.sort_unstable();
                (f.name.as_str(), subjects)
            })
            .filter(|(_, subjects)| !subjects.is_empty())
            .collect();
        fields.sort_unstable_by_key(|(name, _)| *name);
        fields
            .iter()
            .map(|(name, subjects)| format!("{name} ({})", subjects.join(", ")))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// Install the bundled taxonomy, synonyms, goal templates and question banks.
/// Returns the number of taxonomy skills added (0 when every bundled skill was
/// already present).
pub fn install_bundled_data(conn: &Connection) -> Result<i64, String> {
    crate::db::with_transaction(conn, || {
        let skills = install_taxonomy(conn)?;
        conn.execute_batch(GOAL_TEMPLATES_SQL)
            .map_err(|e| format!("install bundled goal templates: {e}"))?;
        conn.execute_batch(QUESTION_BANKS_SQL)
            .map_err(|e| format!("install bundled question banks: {e}"))?;
        Ok(skills)
    })
}

#[cfg(test)]
pub(crate) fn install_foundation(conn: &Connection) -> Result<i64, String> {
    crate::db::with_transaction(conn, || {
        let count = install_taxonomy(conn)?;
        let (synonyms, _) = GOAL_TEMPLATES_SQL
            .split_once("INSERT OR REPLACE INTO goal_templates")
            .ok_or("Bundled synonyms boundary missing")?;
        conn.execute_batch(synonyms).map_err(|e| e.to_string())?;
        Ok(count)
    })
}

pub(crate) fn install_taxonomy(conn: &Connection) -> Result<i64, String> {
    let skill_count = || -> Result<i64, String> {
        conn.query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    };
    let existing_skills = skill_count()?;

    let payload: BootstrapTaxonomyPayload =
        serde_json::from_str(PUBLIC_TAXONOMY_JSON).map_err(|e| e.to_string())?;

    for f in &payload.subject_fields {
        conn.execute(
            "INSERT INTO subject_fields
             (id, name, description, icon_emoji, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, datetime('now')), COALESCE(?6, datetime('now')))
             ON CONFLICT(id) DO UPDATE SET name = excluded.name,
               description = excluded.description, icon_emoji = excluded.icon_emoji",
            params![
                f.id,
                f.name,
                f.description,
                f.icon_emoji,
                f.created_at,
                f.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for s in &payload.subjects {
        conn.execute(
            "INSERT INTO subjects
             (id, name, description, subject_field_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, datetime('now')), COALESCE(?6, datetime('now')))
             ON CONFLICT(id) DO UPDATE SET name = excluded.name,
               description = excluded.description, subject_field_id = excluded.subject_field_id",
            params![
                s.id,
                s.name,
                s.description,
                s.subject_field_id,
                s.created_at,
                s.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for sk in &payload.skills {
        conn.execute(
            "INSERT INTO skills
             (id, name, description, subject_id, bloom_level, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, datetime('now')), COALESCE(?7, datetime('now')))
             ON CONFLICT(id) DO UPDATE SET name = excluded.name,
               description = excluded.description, subject_id = excluded.subject_id,
               bloom_level = excluded.bloom_level",
            params![
                sk.id,
                sk.name,
                sk.description,
                sk.subject_id,
                sk.bloom_level,
                sk.created_at,
                sk.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for edge in &payload.skill_prerequisites {
        conn.execute(
            "INSERT OR IGNORE INTO skill_prerequisites (skill_id, prerequisite_id)
             VALUES (?1, ?2)",
            params![edge.skill_id, edge.prerequisite_id],
        )
        .map_err(|e| e.to_string())?;
    }

    for rel in &payload.skill_relations {
        conn.execute(
            "INSERT OR IGNORE INTO skill_relations (skill_id, related_skill_id, relation_type)
             VALUES (?1, ?2, ?3)",
            params![rel.skill_id, rel.related_skill_id, rel.relation_type],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(skill_count()? - existing_skills)
}

/// Bundled question banks so assessments for these skills work offline.
///
/// Questions are written as `assessment_items`, which is what an attempt
/// actually draws from (`commands::assessment::start_attempt` selects
/// `WHERE bank_id = ?`). Seeding `bank_questions` instead leaves a bank whose
/// item pool is empty, and an empty pool produces an attempt with no
/// questions rather than an error, so the split matters: `content_public` is
/// the half that may reach a client and `grader_private` holds the key.
///
/// `correct_indices` are original-option indices; the runtime shuffles per
/// attempt and grades host-side.
const QUESTION_BANKS_SQL: &str = r#"
INSERT OR IGNORE INTO question_banks (id, skill_id, label, pass_threshold, draw_count, taxonomy_version, ratified) VALUES
  ('qb_js', 'skill_javascript', 'JavaScript fundamentals', 0.7, 3, 'bundled', 1),
  ('qb_bigo', 'skill_big_o', 'Big-O & complexity', 0.7, 3, 'bundled', 1),
  ('qb_one_on_ones', 'skill_one_on_ones', 'One-on-one meetings', 0.7, 3, 'bundled', 1),
  ('qb_giving_feedback', 'skill_giving_feedback', 'Giving feedback', 0.7, 3, 'bundled', 1),
  ('qb_coaching', 'skill_coaching', 'Coaching and mentoring', 0.7, 3, 'bundled', 1),
  ('qb_delegation', 'skill_delegation', 'Delegation', 0.7, 3, 'bundled', 1),
  ('qb_goal_setting', 'skill_goal_setting', 'Goal setting and OKRs', 0.7, 3, 'bundled', 1),
  ('qb_prioritization', 'skill_prioritization', 'Prioritization', 0.7, 3, 'bundled', 1),
  ('qb_effective_meetings', 'skill_effective_meetings', 'Running effective meetings', 0.7, 3, 'bundled', 1),
  ('qb_performance_reviews', 'skill_performance_reviews', 'Performance reviews', 0.7, 3, 'bundled', 1),
  ('qb_hiring_interviews', 'skill_hiring_interviews', 'Hiring and interviewing', 0.7, 3, 'bundled', 1),
  ('qb_stakeholder_comm', 'skill_stakeholder_comm', 'Stakeholder communication', 0.7, 3, 'bundled', 1),
  ('qb_conflict_resolution', 'skill_conflict_resolution', 'Conflict resolution', 0.7, 3, 'bundled', 1),
  ('qb_decision_making', 'skill_decision_making', 'Decision making', 0.7, 3, 'bundled', 1),
  ('qb_change_management', 'skill_change_management', 'Leading change', 0.7, 3, 'bundled', 1);

WITH bundled(id, bank_id, prompt, options, correct_indices, difficulty, points) AS (VALUES
  ('bq_js1', 'qb_js', 'Which keyword declares a block-scoped variable?',
    '["var","let","function","global"]', '[1]', 1, 1.0),
  ('bq_js2', 'qb_js', 'What does `typeof null` return?',
    '["\"null\"","\"object\"","\"undefined\"","throws"]', '[1]', 2, 1.0),
  ('bq_js3', 'qb_js', 'Which are falsy in JavaScript?',
    '["0","\"\"","[]","NaN"]', '[0,1,3]', 3, 1.0),
  ('bq_js4', 'qb_js', 'Which method returns a new array?',
    '["push","map","sort","splice"]', '[1]', 2, 1.0),
  ('bq_bo1', 'qb_bigo', 'Time complexity of binary search?',
    '["O(1)","O(log n)","O(n)","O(n log n)"]', '[1]', 1, 1.0),
  ('bq_bo2', 'qb_bigo', 'Which is asymptotically fastest for large n?',
    '["O(n^2)","O(n log n)","O(n)","O(2^n)"]', '[2]', 2, 1.0),
  ('bq_bo3', 'qb_bigo', 'Average-case lookup in a hash table?',
    '["O(1)","O(log n)","O(n)","O(n log n)"]', '[0]', 2, 1.0),
  ('bq_bo4', 'qb_bigo', 'Which sorts are O(n log n) worst-case?',
    '["quicksort","mergesort","heapsort","bubblesort"]', '[1,2]', 3, 1.0),
  ('bq_one_on_ones1', 'qb_one_on_ones', 'Whose agenda should a regular one-on-one mainly follow?',
    '["The manager''s status checklist", "The report''s topics first", "The project plan", "Whatever came up in standup"]', '[1]', 1, 1.0),
  ('bq_one_on_ones2', 'qb_one_on_ones', 'A report keeps cancelling your weekly one-on-one. What is the best first step?',
    '["Stop scheduling them", "Ask them what would make the time useful", "Escalate to HR", "Turn it into a status meeting"]', '[1]', 2, 1.0),
  ('bq_one_on_ones3', 'qb_one_on_ones', 'Which belong in a healthy one-on-one?',
    '["Career goals", "Blockers and frustrations", "A full project status readout", "Feedback in both directions"]', '[0,1,3]', 2, 1.0),
  ('bq_one_on_ones4', 'qb_one_on_ones', 'Why keep one-on-ones on a fixed, regular schedule?',
    '["Problems surface before they grow", "It replaces written updates", "It lets the manager skip team meetings", "It guarantees a promotion path"]', '[0]', 1, 1.0),
  ('bq_giving_feedback1', 'qb_giving_feedback', 'Which feedback is most useful?',
    '["You need to be more professional", "In Tuesday''s demo you skipped the error cases, and the client asked twice", "Great job lately", "Everyone has noticed your attitude"]', '[1]', 1, 1.0),
  ('bq_giving_feedback2', 'qb_giving_feedback', 'When is corrective feedback usually most effective?',
    '["Soon after the event, in private", "At the annual review", "In the team channel so others learn", "Only once it has happened several times"]', '[0]', 1, 1.0),
  ('bq_giving_feedback3', 'qb_giving_feedback', 'Which make feedback easier to act on?',
    '["Describing the specific behaviour", "Explaining its impact", "Guessing at the person''s motives", "Agreeing on what to do next"]', '[0,1,3]', 2, 1.0),
  ('bq_giving_feedback4', 'qb_giving_feedback', 'Positive feedback should be:',
    '["Saved for performance reviews", "Specific about what worked, so it is repeated", "Avoided so people stay humble", "Always paired with criticism"]', '[1]', 2, 1.0),
  ('bq_coaching1', 'qb_coaching', 'A report asks how to fix a problem they could solve themselves. A coaching response is:',
    '["Fix it for them to save time", "Ask what options they have considered", "Tell them it is not your job", "Assign it to someone else"]', '[1]', 1, 1.0),
  ('bq_coaching2', 'qb_coaching', 'What is the main difference between coaching and mentoring?',
    '["Coaching draws answers out through questions; mentoring shares the mentor''s own experience", "There is no difference", "Mentoring is only for new hires", "Coaching is only for underperformers"]', '[0]', 2, 1.0),
  ('bq_coaching3', 'qb_coaching', 'Which are good coaching questions?',
    '["What have you tried so far?", "What would you do if I were not here?", "Why did you make such a basic mistake?", "What is the first step you could take?"]', '[0,1,3]', 2, 1.0),
  ('bq_coaching4', 'qb_coaching', 'When is telling someone the answer better than coaching?',
    '["Never", "When there is real urgency or risk and no time to explore", "Whenever you already know the answer", "Only with senior staff"]', '[1]', 3, 1.0),
  ('bq_delegation1', 'qb_delegation', 'What must a delegated task include?',
    '["A clear outcome and deadline", "Step-by-step instructions every time", "Daily progress reports", "The manager''s preferred tools"]', '[0]', 1, 1.0),
  ('bq_delegation2', 'qb_delegation', 'How should check-ins on delegated work change with experience?',
    '["Stay the same for everyone", "Become less frequent as the person shows they can handle it", "Become more frequent over time", "Stop entirely after the first task"]', '[1]', 2, 1.0),
  ('bq_delegation3', 'qb_delegation', 'Which are signs a lead is not delegating enough?',
    '["They are the bottleneck for most decisions", "Their team has no stretch work", "Team members own whole projects", "They redo work others have finished"]', '[0,1,3]', 2, 1.0),
  ('bq_delegation4', 'qb_delegation', 'You delegate a task and the result differs from how you would have done it, but it meets the goal. You should:',
    '["Redo it your way", "Accept it and note anything worth sharing", "Take the task back next time", "Ask for it to be redone"]', '[1]', 2, 1.0),
  ('bq_goal_setting1', 'qb_goal_setting', 'Which is a well-formed key result?',
    '["Improve onboarding", "Cut median time to first commit from 10 days to 3", "Work harder on quality", "Launch the new onboarding page"]', '[1]', 1, 1.0),
  ('bq_goal_setting2', 'qb_goal_setting', 'An objective in an OKR should be:',
    '["A list of tasks", "A qualitative, motivating direction", "A single number", "A deadline"]', '[1]', 1, 1.0),
  ('bq_goal_setting3', 'qb_goal_setting', 'Which make team goals easier to own?',
    '["The team helps write them", "Each has a measurable result", "There are as many as possible", "Progress is visible during the quarter"]', '[0,1,3]', 2, 1.0),
  ('bq_goal_setting4', 'qb_goal_setting', 'Halfway through the quarter a key result is clearly wrong. What is the best move?',
    '["Keep it to stay consistent", "Change it openly and explain why", "Quietly drop it", "Add three more key results"]', '[1]', 2, 1.0),
  ('bq_prioritization1', 'qb_prioritization', 'A task is urgent but has little impact. The usual best move is:',
    '["Do it first, always", "Delegate, batch, or time-box it", "Ignore it permanently", "Escalate it"]', '[1]', 1, 1.0),
  ('bq_prioritization2', 'qb_prioritization', 'What does an impact-versus-effort comparison help you find?',
    '["High-impact, low-effort work to do first", "Who to blame for delays", "The team''s velocity", "Exact delivery dates"]', '[0]', 1, 1.0),
  ('bq_prioritization3', 'qb_prioritization', 'Which are healthy ways to say no to new work?',
    '["Show what would be dropped or delayed to fit it in", "Agree, then quietly not do it", "Offer a later date tied to current priorities", "Ask what goal the work serves"]', '[0,2,3]', 2, 1.0),
  ('bq_prioritization4', 'qb_prioritization', 'Your team has ten top priorities. What is the main risk?',
    '["Nothing; ambition is good", "Effort is spread thin and nothing important finishes", "The team will finish early", "Stakeholders will be too happy"]', '[1]', 2, 1.0),
  ('bq_effective_meetings1', 'qb_effective_meetings', 'What should every meeting invitation state?',
    '["The purpose and expected outcome", "Only the time", "The full attendee history", "Nothing; keep it flexible"]', '[0]', 1, 1.0),
  ('bq_effective_meetings2', 'qb_effective_meetings', 'A discussion is going in circles. A good facilitator:',
    '["Lets it run until time is up", "Names the decision needed and who will make it", "Ends the meeting without notes", "Adds more people to the call"]', '[1]', 2, 1.0),
  ('bq_effective_meetings3', 'qb_effective_meetings', 'Which make meeting outcomes stick?',
    '["Written decisions", "Named owners for each action", "Due dates", "A longer meeting next time"]', '[0,1,2]', 2, 1.0),
  ('bq_effective_meetings4', 'qb_effective_meetings', 'When is a meeting usually the wrong tool?',
    '["When the team must make a hard decision together", "When the information is one-way and could be written", "When people disagree", "For a project kickoff"]', '[1]', 2, 1.0),
  ('bq_performance_reviews1', 'qb_performance_reviews', 'What should a performance rating be based on?',
    '["Recent memorable events", "Evidence against expectations agreed in advance", "How much the manager likes the person", "Hours worked"]', '[1]', 1, 1.0),
  ('bq_performance_reviews2', 'qb_performance_reviews', 'Recency bias in reviews means:',
    '["Weighting the last few weeks over the whole period", "Rating new hires lower", "Writing reviews late", "Comparing people with each other"]', '[0]', 2, 1.0),
  ('bq_performance_reviews3', 'qb_performance_reviews', 'Which make reviews fairer?',
    '["Notes kept throughout the period", "Calibration with other managers", "No surprises: issues raised earlier", "Ratings decided before gathering evidence"]', '[0,1,2]', 2, 1.0),
  ('bq_performance_reviews4', 'qb_performance_reviews', 'A review contains a serious concern the person is hearing for the first time. What went wrong?',
    '["Nothing; reviews are for this", "Feedback was not given when the issue arose", "The rating scale", "The review was too short"]', '[1]', 2, 1.0),
  ('bq_hiring_interviews1', 'qb_hiring_interviews', 'What makes an interview structured?',
    '["Each candidate gets the same questions scored against the same rubric", "It follows the candidate''s CV", "It is always technical", "It has many interviewers"]', '[0]', 1, 1.0),
  ('bq_hiring_interviews2', 'qb_hiring_interviews', 'Why write the role''s must-have skills before interviewing?',
    '["So candidates are judged on the job, not on impressions", "To make the job post longer", "It is a legal formality only", "To filter out career changers"]', '[0]', 1, 1.0),
  ('bq_hiring_interviews3', 'qb_hiring_interviews', 'Which reduce bias in hiring decisions?',
    '["Interviewers score independently before discussing", "Decisions based on ''culture fit'' gut feel", "A rubric tied to the role", "Consistent questions for every candidate"]', '[0,2,3]', 2, 1.0),
  ('bq_hiring_interviews4', 'qb_hiring_interviews', 'A behavioural interview question asks:',
    '["How the candidate would hypothetically behave", "For a specific past situation and what the candidate did", "About personal life", "For salary history"]', '[1]', 2, 1.0),
  ('bq_stakeholder_comm1', 'qb_stakeholder_comm', 'A project will miss its date. When should stakeholders hear?',
    '["As soon as you know, with options", "On the date itself", "Only if they ask", "After it ships"]', '[0]', 1, 1.0),
  ('bq_stakeholder_comm2', 'qb_stakeholder_comm', 'A good status update leads with:',
    '["Every task completed", "What changed, what is at risk, and what you need", "The team roster", "A long history of the project"]', '[1]', 1, 1.0),
  ('bq_stakeholder_comm3', 'qb_stakeholder_comm', 'Which help when stakeholders disagree on direction?',
    '["Making the trade-offs explicit", "Naming who decides", "Telling each group what they want to hear", "Writing down the agreed decision"]', '[0,1,3]', 2, 1.0),
  ('bq_stakeholder_comm4', 'qb_stakeholder_comm', 'A stakeholder map helps you:',
    '["Decide who needs what information and how often", "Rank people by seniority", "Avoid talking to anyone", "Set salaries"]', '[0]', 2, 1.0),
  ('bq_conflict_resolution1', 'qb_conflict_resolution', 'Two engineers argue about an approach. What should you separate first?',
    '["Their positions from their underlying interests", "Their desks", "Their projects permanently", "Right from wrong, immediately"]', '[0]', 1, 1.0),
  ('bq_conflict_resolution2', 'qb_conflict_resolution', 'A good opening in a conflict conversation is:',
    '["You are both being unreasonable", "Help me understand what matters most to each of you", "I have already decided", "Let''s not talk about this"]', '[1]', 1, 1.0),
  ('bq_conflict_resolution3', 'qb_conflict_resolution', 'Which tend to de-escalate conflict?',
    '["Restating the other side''s view fairly", "Focusing on shared goals", "Bringing up old grievances", "Agreeing on next steps"]', '[0,1,3]', 2, 1.0),
  ('bq_conflict_resolution4', 'qb_conflict_resolution', 'When should a manager step into a conflict between reports?',
    '["Never; adults should sort it out", "When it blocks work or harms people, after they have tried themselves", "At the first sign of disagreement", "Only in writing"]', '[1]', 2, 1.0),
  ('bq_decision_making1', 'qb_decision_making', 'A two-way-door decision is one that:',
    '["Is easy to reverse, so it can be made quickly", "Needs two approvers", "Can never be undone", "Must go to a vote"]', '[0]', 1, 1.0),
  ('bq_decision_making2', 'qb_decision_making', 'Before a contested decision, what most helps the team accept it?',
    '["Being clear who decides and how input is gathered", "Keeping the process secret", "Deciding by seniority", "Delaying it indefinitely"]', '[0]', 2, 1.0),
  ('bq_decision_making3', 'qb_decision_making', 'Which belong in a written decision record?',
    '["The options considered", "The reasons for the choice", "Who decided", "Who disagreed, named and shamed"]', '[0,1,2]', 2, 1.0),
  ('bq_decision_making4', 'qb_decision_making', 'Disagree and commit means:',
    '["Voicing disagreement, then fully supporting the decision once made", "Pretending to agree", "Blocking until everyone agrees", "Committing only if you agree"]', '[0]', 2, 1.0),
  ('bq_change_management1', 'qb_change_management', 'What do people usually need first to accept a change?',
    '["Understanding why it is happening", "A detailed project plan", "A new tool", "A deadline"]', '[0]', 1, 1.0),
  ('bq_change_management2', 'qb_change_management', 'Resistance to a change is best treated as:',
    '["Information about concerns to address", "Disloyalty", "Something to ignore", "A reason to cancel the change"]', '[0]', 1, 1.0),
  ('bq_change_management3', 'qb_change_management', 'Which help a change stick?',
    '["Early involvement of the people affected", "Visible quick wins", "Announcing it once and moving on", "Updating processes and incentives to match"]', '[0,1,3]', 2, 1.0),
  ('bq_change_management4', 'qb_change_management', 'You announced a reorg by email and people are anxious. A better next step is:',
    '["Send a longer email", "Hold conversations where people can ask what it means for them", "Wait for the anxiety to pass", "Reverse the reorg"]', '[1]', 2, 1.0)
)
INSERT OR IGNORE INTO assessment_items
  (id, item_kind, skill_id, content_public, grader_private,
   difficulty, points, bank_id, taxonomy_version, ratified, bloom_level)
SELECT
  q.id,
  'mcq',
  b.skill_id,
  json_object(
    'kind',    CASE WHEN json_array_length(q.correct_indices) = 1
                    THEN 'single' ELSE 'multi' END,
    'prompt',  q.prompt,
    'options', json(q.options)
  ),
  json_object('correct_indices', json(q.correct_indices)),
  q.difficulty,
  q.points,
  q.bank_id,
  'bundled',
  1,
  'remember'
FROM bundled q
JOIN question_banks b ON b.id = q.bank_id;

-- The primary skill also lands in the multi-skill table, so one query shape
-- serves both the single- and multi-skill cases.
INSERT OR IGNORE INTO assessment_item_skills (item_id, skill_id, weight)
SELECT id, skill_id, 1.0 FROM assessment_items WHERE taxonomy_version = 'bundled';
"#;

/// Skill synonyms for on-device JD and document matching, and bundled goal
/// templates mapping exams, curricula and job roles onto bundled skills.
const GOAL_TEMPLATES_SQL: &str = r#"
-- Synonyms/aliases for on-device JD + document skill matching.
UPDATE skills SET synonyms = 'js,ecmascript,node.js,node' WHERE id = 'skill_javascript';
UPDATE skills SET synonyms = 'ts' WHERE id = 'skill_typescript';
UPDATE skills SET synonyms = 'vue.js,vuejs' WHERE id = 'skill_vue';
UPDATE skills SET synonyms = 'html,css,frontend' WHERE id = 'skill_html_css';
UPDATE skills SET synonyms = 'rest,restful,api,http api' WHERE id = 'skill_rest_api';
UPDATE skills SET synonyms = 'database design,data modeling,sql' WHERE id = 'skill_db_design';
UPDATE skills SET synonyms = 'algorithms,complexity analysis,data structures' WHERE id = 'skill_big_o';
UPDATE skills SET synonyms = 'linear regression,statistical modeling' WHERE id = 'skill_regression';
UPDATE skills SET synonyms = 'neural networks,deep learning,dnn' WHERE id = 'skill_neural_nets';
UPDATE skills SET synonyms = 'supervised learning' WHERE id = 'skill_supervised';
UPDATE skills SET synonyms = 'model evaluation,machine learning' WHERE id = 'skill_ml_eval';
UPDATE skills SET synonyms = 'authentication,oauth,authz' WHERE id = 'skill_auth';
UPDATE skills SET synonyms = 'one on one,one on ones,1 on 1,people management,people manager,manage people,managing people,manager' WHERE id = 'skill_one_on_ones';
UPDATE skills SET synonyms = 'feedback,constructive feedback,difficult feedback' WHERE id = 'skill_giving_feedback';
UPDATE skills SET synonyms = 'coach,coaching,mentor,mentoring,mentorship,develop people,grow people' WHERE id = 'skill_coaching';
UPDATE skills SET synonyms = 'delegate,delegating,lead a team,leading a team,lead teams,team lead,team leader,tech lead' WHERE id = 'skill_delegation';
UPDATE skills SET synonyms = 'okr,okrs,objectives and key results,set goals,goal setting,roadmap,planning' WHERE id = 'skill_goal_setting';
UPDATE skills SET synonyms = 'prioritize,prioritise,prioritisation,priorities,trade offs,tradeoffs,triage' WHERE id = 'skill_prioritization';
UPDATE skills SET synonyms = 'meetings,facilitation,facilitate,standup,stand up,retrospective,retro' WHERE id = 'skill_effective_meetings';
UPDATE skills SET synonyms = 'performance review,performance management,appraisal,appraisals,promotion,calibration' WHERE id = 'skill_performance_reviews';
UPDATE skills SET synonyms = 'hiring,hire,recruiting,recruitment,interviewing,interviews,hiring manager,build a team,grow a team' WHERE id = 'skill_hiring_interviews';
UPDATE skills SET synonyms = 'stakeholders,stakeholder management,communication,status updates,executive communication,influence' WHERE id = 'skill_stakeholder_comm';
UPDATE skills SET synonyms = 'conflict,conflicts,disagreement,difficult conversations,negotiation,mediation' WHERE id = 'skill_conflict_resolution';
UPDATE skills SET synonyms = 'decisions,make decisions,decision making,leadership,leader' WHERE id = 'skill_decision_making';
UPDATE skills SET synonyms = 'change management,lead change,leading change,reorg,reorganization,reorganisation,transformation' WHERE id = 'skill_change_management';

-- Bundled goal templates, labelled taxonomy_version 'bundled'. Replaced by id on
-- every install so a profile picks up changed skill lists.
INSERT OR REPLACE INTO goal_templates (id, kind, key, label, board, grade, skill_ids, taxonomy_version, ratified) VALUES
  ('gt_role_em', 'job_role', 'engineering_manager', 'Engineering Manager', NULL, NULL,
   '["skill_one_on_ones","skill_giving_feedback","skill_delegation","skill_hiring_interviews","skill_performance_reviews","skill_prioritization","skill_big_o","skill_db_design","skill_rest_api"]', 'bundled', 1),
  ('gt_role_fe', 'job_role', 'frontend_engineer', 'Frontend Engineer', NULL, NULL,
   '["skill_javascript","skill_typescript","skill_html_css","skill_vue"]', 'bundled', 1),
  ('gt_role_mle', 'job_role', 'ml_engineer', 'Machine Learning Engineer', NULL, NULL,
   '["skill_regression","skill_neural_nets","skill_supervised","skill_ml_eval","skill_probability"]', 'bundled', 1),
  ('gt_exam_jee', 'exam', 'jee_main', 'JEE Main (Engineering entrance)', NULL, NULL,
   '["skill_logic","skill_sets","skill_combinatorics","skill_probability"]', 'bundled', 1),
  ('gt_cur_cbse10', 'curriculum', 'cbse.grade10', 'CBSE — Grade 10', 'CBSE', '10',
   '["skill_logic","skill_sets","skill_probability"]', 'bundled', 1),
  ('gt_cur_icse10', 'curriculum', 'icse.grade10', 'ICSE — Grade 10', 'ICSE', '10',
   '["skill_logic","skill_sets","skill_probability"]', 'bundled', 1),

  -- Additional job roles
  ('gt_role_be', 'job_role', 'backend_engineer', 'Backend Engineer', NULL, NULL,
   '["skill_rest_api","skill_db_design","skill_sql","skill_docker","skill_ci_cd"]', 'bundled', 1),
  ('gt_role_fs', 'job_role', 'fullstack_engineer', 'Full-Stack Engineer', NULL, NULL,
   '["skill_javascript","skill_react","skill_rest_api","skill_db_design"]', 'bundled', 1),
  ('gt_role_ds', 'job_role', 'data_scientist', 'Data Scientist', NULL, NULL,
   '["skill_python","skill_regression","skill_probability","skill_distributions","skill_inference"]', 'bundled', 1),
  ('gt_role_de', 'job_role', 'data_engineer', 'Data Engineer', NULL, NULL,
   '["skill_etl","skill_sql","skill_streaming","skill_db_design"]', 'bundled', 1),
  ('gt_role_devops', 'job_role', 'devops_engineer', 'DevOps Engineer', NULL, NULL,
   '["skill_docker","skill_ci_cd","skill_dns","skill_concurrency"]', 'bundled', 1),
  ('gt_role_sec', 'job_role', 'security_engineer', 'Security Engineer', NULL, NULL,
   '["skill_symmetric","skill_asymmetric","skill_tls","skill_firewalls","skill_auth"]', 'bundled', 1),
  ('gt_role_pd', 'job_role', 'product_designer', 'Product Designer', NULL, NULL,
   '["skill_ia","skill_design_systems","skill_color_theory","skill_accessibility"]', 'bundled', 1),
  ('gt_role_tl', 'job_role', 'team_lead', 'Team Lead', NULL, NULL,
   '["skill_one_on_ones","skill_giving_feedback","skill_delegation","skill_effective_meetings","skill_prioritization","skill_conflict_resolution"]', 'bundled', 1),
  ('gt_role_people_mgr', 'job_role', 'people_manager', 'People Manager', NULL, NULL,
   '["skill_one_on_ones","skill_coaching","skill_giving_feedback","skill_performance_reviews","skill_hiring_interviews","skill_conflict_resolution"]', 'bundled', 1),
  ('gt_role_pm', 'job_role', 'product_manager', 'Product Manager', NULL, NULL,
   '["skill_prioritization","skill_goal_setting","skill_stakeholder_comm","skill_decision_making","skill_user_research"]', 'bundled', 1),
  ('gt_role_eng_dir', 'job_role', 'engineering_director', 'Engineering Director', NULL, NULL,
   '["skill_goal_setting","skill_decision_making","skill_change_management","skill_stakeholder_comm","skill_hiring_interviews","skill_performance_reviews"]', 'bundled', 1),

  -- Additional exams
  ('gt_exam_gate', 'exam', 'gate_cse', 'GATE — Computer Science', NULL, NULL,
   '["skill_big_o","skill_arrays","skill_graphs","skill_dp","skill_logic"]', 'bundled', 1),
  ('gt_exam_gre', 'exam', 'gre', 'GRE (General)', NULL, NULL,
   '["skill_logic","skill_probability","skill_combinatorics"]', 'bundled', 1),
  ('gt_exam_upsc', 'exam', 'upsc_prelims', 'UPSC Civil Services (Prelims)', NULL, NULL,
   '["skill_constitutional_literacy","skill_federalism_vs_centralism","skill_public_finance_literacy","skill_media_literacy_political"]', 'bundled', 1),

  -- Additional curricula
  ('gt_cur_cbse12', 'curriculum', 'cbse.grade12', 'CBSE — Grade 12', 'CBSE', '12',
   '["skill_derivatives","skill_integrals","skill_probability","skill_logic"]', 'bundled', 1),
  ('gt_cur_icse12', 'curriculum', 'icse.grade12', 'ICSE — Grade 12', 'ICSE', '12',
   '["skill_derivatives","skill_integrals","skill_sets"]', 'bundled', 1),
  ('gt_cur_ib_math', 'curriculum', 'ib.dp.math', 'IB Diploma — Mathematics', 'IB', '12',
   '["skill_derivatives","skill_integrals","skill_matrices","skill_probability"]', 'bundled', 1),
  ('gt_cur_alevel_cs', 'curriculum', 'alevels.cs', 'A-Levels — Computer Science', 'A-Levels', '12',
   '["skill_big_o","skill_arrays","skill_python","skill_logic"]', 'bundled', 1);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const INSTALLED_TABLES: [&str; 8] = [
        "subject_fields",
        "subjects",
        "skills",
        "skill_prerequisites",
        "goal_templates",
        "question_banks",
        "assessment_items",
        "assessment_item_skills",
    ];

    fn migrated() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        db
    }

    fn count(db: &Database, sql: &str) -> i64 {
        db.conn()
            .query_row(sql, [], |row| row.get(0))
            .expect("count rows")
    }

    fn table_counts(db: &Database) -> Vec<i64> {
        INSTALLED_TABLES
            .iter()
            .map(|table| count(db, &format!("SELECT COUNT(*) FROM {table}")))
            .collect()
    }

    #[test]
    fn fresh_profile_gets_only_labelled_built_in_rows() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let payload: BootstrapTaxonomyPayload =
            serde_json::from_str(PUBLIC_TAXONOMY_JSON).expect("bundled taxonomy");
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM skills"),
            payload.skills.len() as i64
        );
        assert!(count(&db, "SELECT COUNT(*) FROM goal_templates") > 0);
        assert!(count(&db, "SELECT COUNT(*) FROM question_banks") > 0);
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM goal_templates WHERE taxonomy_version != 'bundled'"
            ),
            0
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM question_banks WHERE taxonomy_version != 'bundled'"
            ),
            0
        );
        for table in [
            "local_identity",
            "courses",
            "course_elements",
            "enrollments",
            "credentials",
            "key_registry",
            "opinions",
            "classrooms",
            "classroom_members",
            "tutoring_sessions",
            "reputation_assertions",
            "derived_skill_states",
            "pinboard_observations",
            "completion_observations",
            "devices",
            "sync_log",
        ] {
            assert_eq!(
                count(&db, &format!("SELECT COUNT(*) FROM {table}")),
                0,
                "fabricated rows in {table}"
            );
        }
        // The governance DAO, member and proposal tables are not checked for
        // fabricated rows because the baseline schema does not create them.
        // `forbidden_tables_are_absent` asserts that directly.
    }

    #[test]
    fn bundled_templates_and_banks_reference_bundled_skills() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let mut stmt = db
            .conn()
            .prepare("SELECT id, skill_ids FROM goal_templates")
            .expect("prepare templates");
        let templates: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query templates")
            .collect::<Result<_, _>>()
            .expect("read templates");
        for (id, skill_ids) in templates {
            let skill_ids: Vec<String> = serde_json::from_str(&skill_ids).expect("skill ids");
            for skill in skill_ids {
                let present: i64 = db
                    .conn()
                    .query_row(
                        "SELECT COUNT(*) FROM skills WHERE id = ?1",
                        params![skill],
                        |row| row.get(0),
                    )
                    .expect("look up skill");
                assert_eq!(present, 1, "{id} references missing skill {skill}");
            }
        }
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM question_banks WHERE skill_id NOT IN (SELECT id FROM skills)"
            ),
            0
        );
    }

    /// An attempt draws from `assessment_items WHERE bank_id = ?`, so a bank
    /// seeded only into `bank_questions` resolves against `skills` and still
    /// yields an empty draw — an attempt with no questions and no error.
    /// Counting bank rows cannot see that; this counts what the draw sees.
    #[test]
    fn every_bundled_bank_can_fill_a_draw() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let mut stmt = db
            .conn()
            .prepare("SELECT id, draw_count FROM question_banks")
            .expect("prepare banks");
        let banks: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query banks")
            .collect::<Result<_, _>>()
            .expect("read banks");
        assert!(!banks.is_empty(), "no bundled banks installed");

        for (bank_id, draw_count) in banks {
            let items: i64 = db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM assessment_items WHERE bank_id = ?1",
                    params![bank_id],
                    |row| row.get(0),
                )
                .expect("count items");
            assert!(
                items >= draw_count,
                "bank {bank_id} draws {draw_count} but has {items} items"
            );
        }
    }

    #[test]
    fn a_bundled_item_serves_its_prompt_and_withholds_its_key() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let (public, private): (String, String) = db
            .conn()
            .query_row(
                "SELECT content_public, grader_private FROM assessment_items \
                 WHERE id = 'bq_js3'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read bundled item");

        let public: serde_json::Value = serde_json::from_str(&public).expect("public json");
        assert_eq!(public["kind"], "multi");
        assert!(public["prompt"].as_str().expect("prompt").contains("falsy"));
        assert_eq!(public["options"].as_array().expect("options").len(), 4);
        assert!(
            public.get("correct_indices").is_none(),
            "answer key reached the public half: {public}"
        );

        let private: serde_json::Value = serde_json::from_str(&private).expect("private json");
        assert_eq!(private["correct_indices"], serde_json::json!([0, 1, 3]));
    }

    /// Every profile open reinstalls bundled data, so a profile created
    /// before a skill was added gains it, along with its bank and the
    /// templates that use it, and keeps the skills it already had.
    #[test]
    fn a_profile_from_an_older_build_gains_newly_bundled_skills() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");
        let all = count(&db, "SELECT COUNT(*) FROM skills");
        db.conn()
            .execute_batch(
                "DELETE FROM assessment_item_skills WHERE skill_id LIKE 'skill_coaching';
                 DELETE FROM assessment_items WHERE bank_id = 'qb_coaching';
                 DELETE FROM question_banks WHERE id = 'qb_coaching';
                 DELETE FROM skill_prerequisites WHERE 'skill_coaching' IN (skill_id, prerequisite_id);
                 DELETE FROM skill_relations WHERE 'skill_coaching' IN (skill_id, related_skill_id);
                 DELETE FROM skills WHERE id = 'skill_coaching';
                 UPDATE goal_templates SET skill_ids = '[\"skill_big_o\"]' WHERE id = 'gt_role_em';",
            )
            .expect("simulate an older profile");

        assert_eq!(install_bundled_data(db.conn()).expect("reinstall"), 1);

        assert_eq!(count(&db, "SELECT COUNT(*) FROM skills"), all);
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM skills WHERE id = 'skill_coaching' AND synonyms LIKE '%mentoring%'"
            ),
            1
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM assessment_items WHERE bank_id = 'qb_coaching'"
            ),
            4
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM goal_templates WHERE id = 'gt_role_em' AND skill_ids LIKE '%skill_one_on_ones%'"
            ),
            1
        );
    }

    #[test]
    fn the_subject_catalog_lists_every_bundled_field() {
        let catalog = subject_catalog();
        assert!(catalog.starts_with("Civic Engagement ("), "{catalog}");
        assert!(catalog.contains(
            "Leadership & Management (Communication & Influence, People Leadership, Team Management)"
        ));
        let payload: BootstrapTaxonomyPayload =
            serde_json::from_str(PUBLIC_TAXONOMY_JSON).expect("bundled taxonomy");
        assert_eq!(catalog.split("; ").count(), payload.subject_fields.len());
    }

    #[test]
    fn reinstalling_changes_nothing() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");
        let before = table_counts(&db);

        assert_eq!(install_bundled_data(db.conn()).expect("reinstall"), 0);

        assert_eq!(table_counts(&db), before);
    }

    #[test]
    fn failed_install_leaves_no_partial_rows_and_can_retry() {
        let db = migrated();
        db.conn()
            .execute_batch(
                "CREATE TEMP TRIGGER fail_bundled_bank BEFORE INSERT ON assessment_items \
                 BEGIN SELECT RAISE(ABORT, 'injected bank failure'); END;",
            )
            .expect("install fault");

        let error = install_bundled_data(db.conn()).unwrap_err();

        assert!(error.contains("injected bank failure"), "{error}");
        assert!(table_counts(&db).iter().all(|rows| *rows == 0));
        db.conn()
            .execute_batch("DROP TRIGGER fail_bundled_bank")
            .expect("remove fault");
        assert!(install_bundled_data(db.conn()).expect("retry install") > 0);
    }
}
