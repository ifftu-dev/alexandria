use serde::{Deserialize, Serialize};

/// A course in the local database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Course {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub author_address: String,
    pub author_name: Option<String>,
    pub content_cid: Option<String>,
    pub thumbnail_cid: Option<String>,
    pub thumbnail_svg: Option<String>,
    pub tags: Option<Vec<String>>,
    pub skill_ids: Option<Vec<String>>,
    pub version: i64,
    pub status: String,
    pub published_at: Option<String>,
    pub on_chain_tx: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// `"course"` or `"tutorial"`. Defaults to `"course"` for rows
    /// that predate migration 020.
    #[serde(default = "default_course_kind")]
    pub kind: String,
    /// Where this content came from. `"ai_generated"` marks seeded
    /// example content; `None` means user-created. Free-form TEXT so
    /// future provenance values do not require a schema change.
    /// Added in migration 031.
    #[serde(default)]
    pub provenance: Option<String>,
}

fn default_course_kind() -> String {
    "course".to_string()
}

/// Request to create a new course.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCourseRequest {
    pub title: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub skill_ids: Option<Vec<String>>,
}

/// Request to update an existing course.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCourseRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub skill_ids: Option<Vec<String>>,
    pub status: Option<String>,
}

/// A chapter within a course.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub course_id: String,
    pub title: String,
    pub description: Option<String>,
    pub position: i64,
}

/// Request to create a new chapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChapterRequest {
    pub title: String,
    pub description: Option<String>,
}

/// Request to update an existing chapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChapterRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub position: Option<i64>,
}

/// An element (learning item) within a chapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    pub chapter_id: String,
    pub title: String,
    pub element_type: String,
    pub content_cid: Option<String>,
    pub content_inline: Option<String>,
    pub position: i64,
    pub duration_seconds: Option<i64>,
}

/// Request to create a new element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateElementRequest {
    pub title: String,
    pub element_type: String,
    pub content_hash: Option<String>,
    pub duration_seconds: Option<i64>,
}

/// Request to update an existing element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateElementRequest {
    pub title: Option<String>,
    pub element_type: Option<String>,
    pub content_hash: Option<String>,
    pub position: Option<i64>,
    pub duration_seconds: Option<i64>,
}

/// A single video chapter marker (title + start second), used when
/// creating a tutorial so the timestamp navigation is authored in the
/// same call as the tutorial itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoChapterInput {
    pub title: String,
    pub start_seconds: i64,
}

/// Optional end-of-video quiz attached to a tutorial. When present,
/// it becomes a second `course_elements` row of `element_type='quiz'`
/// with the same skill tags — this is what lets watching + passing
/// the quiz feed the evidence pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TutorialQuizInput {
    /// Quiz body — JSON matching the existing quiz element format
    /// (questions, options, correct answers). Stored inline in the
    /// `course_elements.content_inline` column.
    pub content_json: String,
}

/// Skill tag on a tutorial: a skill ID plus the weight with which
/// completing the tutorial contributes to evidence for that skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTagInput {
    pub skill_id: String,
    /// Weight in [0.0, 1.0]. Defaults to 1.0 at the DB layer.
    pub weight: Option<f64>,
}

/// Everything needed to publish a standalone video tutorial in one shot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishTutorialRequest {
    pub title: String,
    pub description: Option<String>,
    /// BLAKE3 content hash of the uploaded video blob (produced via
    /// `content_add`). Must already exist in the iroh store.
    pub video_content_hash: String,
    /// Optional BLAKE3 thumbnail hash.
    pub thumbnail_hash: Option<String>,
    /// Video duration in seconds (for UI display + trust_factor logic).
    pub duration_seconds: Option<i64>,
    /// At least one skill tag is required — a tutorial without a
    /// skill is just a video, not a learning artefact.
    pub skill_tags: Vec<SkillTagInput>,
    /// Optional chapter markers for timestamp navigation.
    #[serde(default)]
    pub video_chapters: Vec<VideoChapterInput>,
    /// Optional end-of-video quiz (grants partial skill evidence on pass).
    pub quiz: Option<TutorialQuizInput>,
    /// Free-text tags for discovery.
    #[serde(default)]
    pub tags: Vec<String>,
}
