use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{self, DbPool, SkillInstallation};
use crate::AppState;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Result of a single skill install operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub symlink_path: String,
}

/// Result of a batch install across multiple agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchInstallResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<FailedInstall>,
}

/// Describes a single failed install within a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedInstall {
    pub agent_id: String,
    pub error: String,
}

// ─── Path Utilities ───────────────────────────────────────────────────────────

/// Compute a relative path from `from_dir` to `to_path`.
///
/// Both paths must be absolute. The resulting path can be used as a symlink
/// target placed inside `from_dir`.
///
/// Examples:
/// - `make_relative_path("/a/b/c", "/a/d/e/f")` -> `"../../d/e/f"`
/// - `make_relative_path("/home/user/.claude/skills", "/home/user/.agents/skills/my-skill")`
///   -> `"../../.agents/skills/my-skill"`
pub fn make_relative_path(from_dir: &Path, to_path: &Path) -> PathBuf {
    let from_components: Vec<_> = from_dir.components().collect();
    let to_components: Vec<_> = to_path.components().collect();

    // Find the length of the common path prefix.
    let common_len = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Number of ".." hops needed to climb out of `from_dir`.
    let up_count = from_components.len() - common_len;

    let mut result = PathBuf::new();
    for _ in 0..up_count {
        result.push("..");
    }
    for component in &to_components[common_len..] {
        result.push(component.as_os_str());
    }

    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

// ─── Platform-specific symlink creation ──────────────────────────────────────

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| format!("Failed to create symlink: {}", e))
}

#[cfg(not(unix))]
fn create_symlink(_target: &Path, _link: &Path) -> Result<(), String> {
    Err("Symlink creation is only supported on Unix systems".to_string())
}

// ─── Core Logic ───────────────────────────────────────────────────────────────

/// Core install logic, separated from the Tauri layer for testability.
///
/// Creates a relative symlink at `agent.global_skills_dir/<skill_id>` that
/// points to the canonical skill directory `central.global_skills_dir/<skill_id>`.
///
/// Returns an error if:
/// - The agent or central agent is not found in the database.
/// - The canonical skill does not exist (no SKILL.md).
/// - A real (non-symlink) directory already exists at the target path.
/// - `agent_id` is "central" (would create a self-referencing symlink).
pub async fn install_skill_to_agent_impl(
    pool: &DbPool,
    skill_id: &str,
    agent_id: &str,
) -> Result<InstallResult, String> {
    // Guard: cannot install to the central agent itself.
    if agent_id == "central" {
        return Err("Cannot install a skill to the central agent itself".to_string());
    }

    // 1. Look up the target agent.
    let agent = db::get_agent_by_id(pool, agent_id)
        .await?
        .ok_or_else(|| format!("Agent '{}' not found", agent_id))?;

    // 2. Look up the central agent to determine the canonical root.
    let central = db::get_agent_by_id(pool, "central")
        .await?
        .ok_or_else(|| "Central agent not found in database".to_string())?;

    let canonical_dir = PathBuf::from(&central.global_skills_dir).join(skill_id);

    // 3. Verify that the canonical skill directory exists with a SKILL.md.
    if !canonical_dir.join("SKILL.md").exists() {
        return Err(format!(
            "Canonical skill '{}' not found at '{}'",
            skill_id,
            canonical_dir.display()
        ));
    }

    // 4. Compute symlink location.
    let agent_dir = PathBuf::from(&agent.global_skills_dir);
    let symlink_path = agent_dir.join(skill_id);

    // 5. Ensure the agent's skills directory exists.
    std::fs::create_dir_all(&agent_dir)
        .map_err(|e| format!("Failed to create agent skills directory: {}", e))?;

    // 6. Handle any existing entry at the symlink path.
    match std::fs::symlink_metadata(&symlink_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            // Remove stale symlink so we can replace it.
            std::fs::remove_file(&symlink_path)
                .map_err(|e| format!("Failed to remove existing symlink: {}", e))?;
        }
        Ok(meta) if meta.is_dir() => {
            return Err(format!(
                "A real directory already exists at '{}'. Refusing to overwrite.",
                symlink_path.display()
            ));
        }
        Ok(_) => {
            return Err(format!(
                "A file already exists at '{}'. Refusing to overwrite.",
                symlink_path.display()
            ));
        }
        Err(_) => {} // Path does not exist — proceed normally.
    }

    // 7. Compute the relative path from the agent directory to the canonical dir.
    let relative_target = make_relative_path(&agent_dir, &canonical_dir);

    // 8. Create the symlink.
    create_symlink(&relative_target, &symlink_path)?;

    // 9. Persist the installation record.
    let installation = SkillInstallation {
        skill_id: skill_id.to_string(),
        agent_id: agent_id.to_string(),
        installed_path: symlink_path.to_string_lossy().into_owned(),
        link_type: "symlink".to_string(),
        symlink_target: Some(canonical_dir.to_string_lossy().into_owned()),
    };
    db::upsert_skill_installation(pool, &installation).await?;

    Ok(InstallResult {
        symlink_path: symlink_path.to_string_lossy().into_owned(),
    })
}

/// Core uninstall logic, separated from the Tauri layer for testability.
///
/// Removes the symlink at `agent.global_skills_dir/<skill_id>` and deletes the
/// corresponding `skill_installations` record.
///
/// Returns an error if the path exists but is **not** a symlink (refuses to
/// delete real directories).
pub async fn uninstall_skill_from_agent_impl(
    pool: &DbPool,
    skill_id: &str,
    agent_id: &str,
) -> Result<(), String> {
    // 1. Look up the agent.
    let agent = db::get_agent_by_id(pool, agent_id)
        .await?
        .ok_or_else(|| format!("Agent '{}' not found", agent_id))?;

    // 2. Compute the expected symlink location.
    let symlink_path = PathBuf::from(&agent.global_skills_dir).join(skill_id);

    // 3. Inspect the entry at that path.
    match std::fs::symlink_metadata(&symlink_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            std::fs::remove_file(&symlink_path)
                .map_err(|e| format!("Failed to remove symlink: {}", e))?;
        }
        Ok(_) => {
            return Err(format!(
                "Path '{}' exists but is not a symlink. Refusing to delete.",
                symlink_path.display()
            ));
        }
        Err(_) => {
            // Path doesn't exist — still clean up the DB record.
        }
    }

    // 4. Remove the installation record from the database.
    db::delete_skill_installation(pool, skill_id, agent_id).await?;

    Ok(())
}

// ─── Tauri Commands ───────────────────────────────────────────────────────────

/// Tauri command: install a skill to a single agent via relative symlink.
#[tauri::command]
pub async fn install_skill_to_agent(
    state: State<'_, AppState>,
    skill_id: String,
    agent_id: String,
) -> Result<InstallResult, String> {
    install_skill_to_agent_impl(&state.db, &skill_id, &agent_id).await
}

/// Tauri command: remove a skill's symlink from an agent.
#[tauri::command]
pub async fn uninstall_skill_from_agent(
    state: State<'_, AppState>,
    skill_id: String,
    agent_id: String,
) -> Result<(), String> {
    uninstall_skill_from_agent_impl(&state.db, &skill_id, &agent_id).await
}

/// Tauri command: install a skill to multiple agents in one call.
///
/// Each agent install is attempted independently; failures are collected in the
/// `failed` list rather than short-circuiting the entire batch.
#[tauri::command]
pub async fn batch_install_to_agents(
    state: State<'_, AppState>,
    skill_id: String,
    agent_ids: Vec<String>,
) -> Result<BatchInstallResult, String> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for agent_id in &agent_ids {
        match install_skill_to_agent_impl(&state.db, &skill_id, agent_id).await {
            Ok(_) => succeeded.push(agent_id.clone()),
            Err(e) => failed.push(FailedInstall {
                agent_id: agent_id.clone(),
                error: e,
            }),
        }
    }

    Ok(BatchInstallResult { succeeded, failed })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use sqlx::SqlitePool;
    use std::fs;
    use tempfile::TempDir;

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Create an in-memory SQLite pool with the full schema initialised and
    /// the central/claude-code agent directories redirected to `central_dir`
    /// and `agent_dir` respectively.
    async fn setup_db(central_dir: &Path, agent_dir: &Path) -> DbPool {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        db::init_database(&pool).await.unwrap();

        sqlx::query("UPDATE agents SET global_skills_dir = ? WHERE id = 'central'")
            .bind(central_dir.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("UPDATE agents SET global_skills_dir = ? WHERE id = 'claude-code'")
            .bind(agent_dir.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    /// Create a minimal skill directory containing a valid `SKILL.md`.
    fn create_central_skill(central_dir: &Path, skill_id: &str) -> PathBuf {
        let skill_dir = central_dir.join(skill_id);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {}\ndescription: Test skill\n---\n\n# {}\n",
                skill_id, skill_id
            ),
        )
        .unwrap();
        skill_dir
    }

    // ── make_relative_path ────────────────────────────────────────────────────

    #[test]
    fn test_make_relative_path_sibling_dirs() {
        let from = Path::new("/home/user/claude/skills");
        let to = Path::new("/home/user/.agents/skills/my-skill");
        let rel = make_relative_path(from, to);
        assert_eq!(rel, PathBuf::from("../../.agents/skills/my-skill"));
    }

    #[test]
    fn test_make_relative_path_same_parent() {
        let from = Path::new("/tmp/test/agent");
        let to = Path::new("/tmp/test/central/skill-x");
        let rel = make_relative_path(from, to);
        assert_eq!(rel, PathBuf::from("../central/skill-x"));
    }

    #[test]
    fn test_make_relative_path_deep_nesting() {
        let from = Path::new("/a/b/c/d");
        let to = Path::new("/a/x/y");
        let rel = make_relative_path(from, to);
        assert_eq!(rel, PathBuf::from("../../../x/y"));
    }

    // ── install_skill_to_agent_impl ───────────────────────────────────────────

    #[tokio::test]
    async fn test_install_creates_symlink() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;

        create_central_skill(&central_dir, "my-skill");

        let result = install_skill_to_agent_impl(&pool, "my-skill", "claude-code").await;
        assert!(result.is_ok(), "install should succeed: {:?}", result);

        let symlink_path = agent_dir.join("my-skill");
        let meta = fs::symlink_metadata(&symlink_path).unwrap();
        assert!(meta.file_type().is_symlink(), "entry should be a symlink");
    }

    #[tokio::test]
    async fn test_install_symlink_is_relative() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "rel-skill");

        install_skill_to_agent_impl(&pool, "rel-skill", "claude-code")
            .await
            .unwrap();

        let symlink_path = agent_dir.join("rel-skill");
        let link_target = fs::read_link(&symlink_path).unwrap();
        assert!(
            link_target.is_relative(),
            "symlink target should be relative, got {:?}",
            link_target
        );
    }

    #[tokio::test]
    async fn test_install_symlink_resolves_correctly() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "resolve-skill");

        install_skill_to_agent_impl(&pool, "resolve-skill", "claude-code")
            .await
            .unwrap();

        let symlink_path = agent_dir.join("resolve-skill");
        // Following the symlink should give access to SKILL.md in the central dir.
        let skill_md = symlink_path.join("SKILL.md");
        assert!(skill_md.exists(), "SKILL.md should be accessible via symlink");
    }

    #[tokio::test]
    async fn test_install_creates_agent_dir_if_missing() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        // Do NOT pre-create agent_dir — install should create it.
        let agent_dir = tmp.path().join("new-agent-dir");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "dir-skill");

        let result = install_skill_to_agent_impl(&pool, "dir-skill", "claude-code").await;
        assert!(result.is_ok(), "install should create missing agent dir");
        assert!(agent_dir.exists(), "agent dir should have been created");
    }

    #[tokio::test]
    async fn test_install_updates_db_record() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "db-skill");

        install_skill_to_agent_impl(&pool, "db-skill", "claude-code")
            .await
            .unwrap();

        let installations = db::get_skill_installations(&pool, "db-skill").await.unwrap();
        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0].agent_id, "claude-code");
        assert_eq!(installations[0].link_type, "symlink");
    }

    #[tokio::test]
    async fn test_install_fails_when_canonical_missing() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        // Do NOT create the skill in central_dir.

        let result = install_skill_to_agent_impl(&pool, "nonexistent-skill", "claude-code").await;
        assert!(result.is_err(), "install should fail if canonical skill missing");
    }

    #[tokio::test]
    async fn test_install_fails_for_unknown_agent() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "some-skill");

        let result =
            install_skill_to_agent_impl(&pool, "some-skill", "nonexistent-agent").await;
        assert!(result.is_err(), "install should fail for unknown agent");
    }

    #[tokio::test]
    async fn test_install_to_central_agent_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &tmp.path().join("claude")).await;
        create_central_skill(&central_dir, "self-skill");

        let result = install_skill_to_agent_impl(&pool, "self-skill", "central").await;
        assert!(result.is_err(), "installing to 'central' should be rejected");
    }

    #[tokio::test]
    async fn test_install_replaces_existing_symlink() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();
        fs::create_dir_all(&agent_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "re-link-skill");

        // Install once.
        install_skill_to_agent_impl(&pool, "re-link-skill", "claude-code")
            .await
            .unwrap();

        // Install again — should replace the existing symlink without error.
        let result =
            install_skill_to_agent_impl(&pool, "re-link-skill", "claude-code").await;
        assert!(result.is_ok(), "re-install should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_install_refuses_to_overwrite_real_dir() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();
        fs::create_dir_all(&agent_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "real-dir-skill");

        // Create a real (non-symlink) directory at the install location.
        fs::create_dir_all(agent_dir.join("real-dir-skill")).unwrap();

        let result =
            install_skill_to_agent_impl(&pool, "real-dir-skill", "claude-code").await;
        assert!(
            result.is_err(),
            "install should refuse to overwrite a real directory"
        );
    }

    // ── uninstall_skill_from_agent_impl ───────────────────────────────────────

    #[tokio::test]
    async fn test_uninstall_removes_symlink() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "uninstall-skill");

        install_skill_to_agent_impl(&pool, "uninstall-skill", "claude-code")
            .await
            .unwrap();

        let symlink_path = agent_dir.join("uninstall-skill");
        assert!(symlink_path.exists() || fs::symlink_metadata(&symlink_path).is_ok());

        uninstall_skill_from_agent_impl(&pool, "uninstall-skill", "claude-code")
            .await
            .unwrap();

        assert!(
            fs::symlink_metadata(&symlink_path).is_err(),
            "symlink should have been removed"
        );
    }

    #[tokio::test]
    async fn test_uninstall_removes_db_record() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;
        create_central_skill(&central_dir, "db-uninstall-skill");

        install_skill_to_agent_impl(&pool, "db-uninstall-skill", "claude-code")
            .await
            .unwrap();

        uninstall_skill_from_agent_impl(&pool, "db-uninstall-skill", "claude-code")
            .await
            .unwrap();

        let installations = db::get_skill_installations(&pool, "db-uninstall-skill")
            .await
            .unwrap();
        assert!(installations.is_empty(), "DB record should be removed");
    }

    #[tokio::test]
    async fn test_uninstall_refuses_real_dir() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;

        // Place a real directory where the symlink would be.
        fs::create_dir_all(agent_dir.join("protected-skill")).unwrap();

        let result =
            uninstall_skill_from_agent_impl(&pool, "protected-skill", "claude-code").await;
        assert!(
            result.is_err(),
            "uninstall should refuse to delete a real directory"
        );

        // Ensure the directory still exists.
        assert!(
            agent_dir.join("protected-skill").is_dir(),
            "real directory should NOT have been deleted"
        );
    }

    #[tokio::test]
    async fn test_uninstall_nonexistent_path_still_cleans_db() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let agent_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();
        fs::create_dir_all(&agent_dir).unwrap();

        let pool = setup_db(&central_dir, &agent_dir).await;

        // Manually insert an installation record without creating the symlink.
        let installation = SkillInstallation {
            skill_id: "ghost-skill".to_string(),
            agent_id: "claude-code".to_string(),
            installed_path: agent_dir.join("ghost-skill").to_string_lossy().into_owned(),
            link_type: "symlink".to_string(),
            symlink_target: None,
        };
        db::upsert_skill_installation(&pool, &installation).await.unwrap();

        let result =
            uninstall_skill_from_agent_impl(&pool, "ghost-skill", "claude-code").await;
        assert!(result.is_ok(), "uninstall of missing path should succeed");

        let installations = db::get_skill_installations(&pool, "ghost-skill")
            .await
            .unwrap();
        assert!(installations.is_empty(), "DB record should be cleaned up");
    }

    // ── batch install ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_batch_install_multiple_agents() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let claude_dir = tmp.path().join("claude");
        let cursor_dir = tmp.path().join("cursor");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &claude_dir).await;

        // Override cursor's dir too.
        sqlx::query("UPDATE agents SET global_skills_dir = ? WHERE id = 'cursor'")
            .bind(cursor_dir.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();

        create_central_skill(&central_dir, "batch-skill");

        let result = batch_install_impl(
            &pool,
            "batch-skill",
            &["claude-code".to_string(), "cursor".to_string()],
        )
        .await;

        assert_eq!(result.succeeded.len(), 2);
        assert!(result.failed.is_empty());

        assert!(fs::symlink_metadata(claude_dir.join("batch-skill")).is_ok());
        assert!(fs::symlink_metadata(cursor_dir.join("batch-skill")).is_ok());
    }

    #[tokio::test]
    async fn test_batch_install_partial_failure() {
        let tmp = TempDir::new().unwrap();
        let central_dir = tmp.path().join("central");
        let claude_dir = tmp.path().join("claude");
        fs::create_dir_all(&central_dir).unwrap();

        let pool = setup_db(&central_dir, &claude_dir).await;
        create_central_skill(&central_dir, "partial-skill");

        let result = batch_install_impl(
            &pool,
            "partial-skill",
            &[
                "claude-code".to_string(),
                "nonexistent-agent".to_string(), // will fail
            ],
        )
        .await;

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].agent_id, "nonexistent-agent");
    }

    /// Helper that mirrors `batch_install_to_agents` but works with a raw pool
    /// (no Tauri State).
    async fn batch_install_impl(
        pool: &DbPool,
        skill_id: &str,
        agent_ids: &[String],
    ) -> BatchInstallResult {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for agent_id in agent_ids {
            match install_skill_to_agent_impl(pool, skill_id, agent_id).await {
                Ok(_) => succeeded.push(agent_id.clone()),
                Err(e) => failed.push(FailedInstall {
                    agent_id: agent_id.clone(),
                    error: e,
                }),
            }
        }

        BatchInstallResult { succeeded, failed }
    }
}
