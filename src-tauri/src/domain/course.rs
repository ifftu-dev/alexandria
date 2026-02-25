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
