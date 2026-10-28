use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

impl PackageManager {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeProjectConfig {
    pub package_manager: PackageManager,
    pub install_command: String,
    pub build_command: Option<String>,
    pub start_command: String,
}

pub struct NodeDetector;

impl NodeDetector {
    pub fn detect_from_package_json(content: &str) -> Result<NodeProjectConfig, ApiError> {
        let json: Value = serde_json::from_str(content)
            .map_err(|err| ApiError::BadRequest(format!("Failed to parse package.json: {err}")))?;

        // 1. Detect package manager
        let pm = if let Some(pm_str) = json.get("packageManager").and_then(|v| v.as_str()) {
            if pm_str.starts_with("pnpm") {
                PackageManager::Pnpm
            } else if pm_str.starts_with("yarn") {
                PackageManager::Yarn
            } else {
                PackageManager::Npm
            }
        } else {
            PackageManager::Npm
        };

        // 2. Resolve install command
        let install_command = match pm {
            PackageManager::Npm => "npm ci".to_string(),
            PackageManager::Pnpm => "pnpm install --frozen-lockfile".to_string(),
            PackageManager::Yarn => "yarn install --frozen-lockfile".to_string(),
        };

        // 3. Resolve build command
        let scripts = json.get("scripts").and_then(|v| v.as_object());
        let build_command = scripts
            .and_then(|s| s.get("build"))
            .and_then(|b| b.as_str())
            .map(|_| format!("{} run build", pm.as_str()));

        // 4. Resolve start command
        let start_command = if let Some(s) = scripts
            .and_then(|s| s.get("start"))
            .and_then(|v| v.as_str())
        {
            if !s.trim().is_empty() {
                format!("{} start", pm.as_str())
            } else {
                Self::fallback_start(&json)
            }
        } else {
            Self::fallback_start(&json)
        };

        Ok(NodeProjectConfig {
            package_manager: pm,
            install_command,
            build_command,
            start_command,
        })
    }

    fn fallback_start(json: &Value) -> String {
        if let Some(main) = json.get("main").and_then(|v| v.as_str()) {
            format!("node {main}")
        } else {
            "node index.js".to_string()
        }
    }
}
