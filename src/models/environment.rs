use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Environment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Environment {
    pub fn default_production(project_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            project_id,
            name: "production".to_string(),
            created_at: Utc::now(),
        }
    }
}
