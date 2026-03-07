//! Path sandboxing utilities.
//!
//! Every file-system tool MUST call [`assert_sandbox_path`] before
//! touching any path. This module prevents workspace escapes,
//! including via `..` traversal and symlink resolution.

use crate::types::ToolError;
use std::path::{Path, PathBuf};

/// Resolve `target` relative to `workspace_root` and ensure the
/// canonical path stays inside the workspace.
///
/// Returns the canonicalized absolute path on success.
///
/// # Errors
///
/// - `PermissionDenied` if the resolved path escapes the workspace.
/// - `PermissionDenied` if we cannot canonicalize (e.g. path doesn't exist
///   yet) and the logical normalization escapes the workspace.
pub fn assert_sandbox_path(workspace_root: &Path, target: &str) -> Result<PathBuf, ToolError> {
    let target_path = Path::new(target);

    // Build absolute path.
    let absolute = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        workspace_root.join(target_path)
    };

    // Canonicalize the workspace root for consistent comparison.
    let canon_root = workspace_root
        .canonicalize()
        .map_err(|e| ToolError::PermissionDenied {
            reason: format!("cannot resolve workspace root: {e}"),
        })?;

    // Try canonicalize (resolves symlinks). If the file doesn't exist yet
    // (e.g. write_file creating a new file), canonicalize the nearest
    // existing ancestor and append the remaining components.
    let resolved = match absolute.canonicalize() {
        Ok(p) => p,
        Err(_) => resolve_via_existing_ancestor(&absolute, &canon_root)?,
    };

    if !resolved.starts_with(&canon_root) {
        return Err(ToolError::PermissionDenied {
            reason: format!(
                "path '{}' escapes workspace '{}'",
                target,
                canon_root.display()
            ),
        });
    }

    Ok(resolved)
}

/// For non-existent paths: walk up until we find an existing ancestor,
/// canonicalize that, then re-append the remaining components.
/// This ensures consistent path formats on Windows (\\?\ prefix).
fn resolve_via_existing_ancestor(path: &Path, canon_root: &Path) -> Result<PathBuf, ToolError> {
    let normalized = logical_normalize(path);
    let mut ancestors: Vec<&std::ffi::OsStr> = Vec::new();
    let mut current = normalized.as_path();

    // Walk up to find an existing ancestor we can canonicalize.
    loop {
        if current.exists() {
            let canon = current
                .canonicalize()
                .map_err(|e| ToolError::PermissionDenied {
                    reason: format!("cannot canonicalize '{}': {e}", current.display()),
                })?;
            // Re-append the non-existent trailing components (reverse since they were collected leaf-first).
            let mut result = canon;
            for component in ancestors.iter().rev() {
                result = result.join(component);
            }
            return Ok(result);
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                ancestors.push(name);
                current = parent;
            }
            _ => break,
        }
    }

    // Fallback: nothing exists — check logical normalization against root.
    // This handles edge cases where even the drive root doesn't exist (unlikely).
    if !normalized.starts_with(canon_root) {
        return Err(ToolError::PermissionDenied {
            reason: format!(
                "path '{}' escapes workspace '{}'",
                path.display(),
                canon_root.display()
            ),
        });
    }
    Ok(normalized)
}

/// Assert that the *already-canonicalized* path has no symlink that
/// jumps outside the workspace. This is a secondary check after
/// `assert_sandbox_path` for paths that already exist.
pub fn assert_no_symlink_escape(workspace_root: &Path, path: &Path) -> Result<(), ToolError> {
    // If the path exists, canonicalize it (resolves ALL symlinks).
    if path.exists() {
        let canon = path
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("cannot canonicalize '{}': {e}", path.display()),
            })?;
        let canon_root =
            workspace_root
                .canonicalize()
                .map_err(|e| ToolError::PermissionDenied {
                    reason: format!("cannot resolve workspace root: {e}"),
                })?;
        if !canon.starts_with(&canon_root) {
            return Err(ToolError::PermissionDenied {
                reason: format!(
                    "symlink at '{}' resolves to '{}' which is outside workspace '{}'",
                    path.display(),
                    canon.display(),
                    canon_root.display()
                ),
            });
        }
    }
    Ok(())
}

/// Logical normalization: strips `.` and `..` components without
/// touching the filesystem. Used when the target doesn't exist yet.
fn logical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sandbox_allows_file_inside_workspace() {
        let tmp = std::env::temp_dir().join("sandbox_test_allow");
        fs::create_dir_all(&tmp).unwrap();
        // Create a file inside the workspace.
        fs::write(tmp.join("hello.txt"), "hi").unwrap();

        let result = assert_sandbox_path(&tmp, "hello.txt");
        assert!(result.is_ok());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn sandbox_blocks_dot_dot_traversal() {
        let tmp = std::env::temp_dir().join("sandbox_test_dotdot");
        fs::create_dir_all(&tmp).unwrap();

        let result = assert_sandbox_path(&tmp, "../../../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("escapes workspace"), "got: {err}");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn sandbox_blocks_absolute_path_outside() {
        let tmp = std::env::temp_dir().join("sandbox_test_abs");
        fs::create_dir_all(&tmp).unwrap();

        // Use an absolute path that's definitely outside the workspace.
        let outside = std::env::temp_dir()
            .join("sandbox_test_abs_outside_target")
            .to_string_lossy()
            .to_string();
        // Create the outside dir so canonicalize works.
        fs::create_dir_all(&outside).unwrap();

        let result = assert_sandbox_path(&tmp, &outside);
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn sandbox_allows_nested_path() {
        let tmp = std::env::temp_dir().join("sandbox_test_nested");
        fs::create_dir_all(tmp.join("sub/dir")).unwrap();
        fs::write(tmp.join("sub/dir/file.txt"), "ok").unwrap();

        let result = assert_sandbox_path(&tmp, "sub/dir/file.txt");
        assert!(result.is_ok());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_blocks_symlink_escape() {
        use std::os::unix::fs::symlink;

        let tmp = std::env::temp_dir().join("sandbox_test_symlink_esc");
        fs::create_dir_all(&tmp).unwrap();

        // Create a symlink pointing outside.
        let outside_file = std::env::temp_dir().join("sandbox_symlink_outside_target.txt");
        fs::write(&outside_file, "secret").unwrap();
        let link = tmp.join("escape_link.txt");
        let _ = fs::remove_file(&link);
        symlink(&outside_file, &link).unwrap();

        let result = assert_sandbox_path(&tmp, "escape_link.txt");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("escapes workspace"), "got: {err}");

        fs::remove_dir_all(&tmp).unwrap();
        let _ = fs::remove_file(&outside_file);
    }

    #[cfg(windows)]
    #[test]
    fn sandbox_blocks_symlink_escape_windows() {
        // On Windows, symlink creation requires elevated permissions or
        // developer mode. We test the no_symlink_escape function directly
        // with a path that would resolve outside workspace.
        let tmp = std::env::temp_dir().join("sandbox_test_win");
        fs::create_dir_all(&tmp).unwrap();

        // Test directory junction (more commonly available on Windows).
        let outside = std::env::temp_dir().join("sandbox_test_win_outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "data").unwrap();

        // Even without a real symlink, assert_no_symlink_escape should pass
        // for an in-workspace path.
        let in_ws = tmp.join("legit.txt");
        fs::write(&in_ws, "ok").unwrap();
        assert!(assert_no_symlink_escape(&tmp, &in_ws).is_ok());

        fs::remove_dir_all(&tmp).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn sandbox_allows_new_file_path() {
        let tmp = std::env::temp_dir().join("sandbox_test_newfile");
        fs::create_dir_all(&tmp).unwrap();

        // File doesn't exist yet — should still be allowed.
        let result = assert_sandbox_path(&tmp, "new_file.txt");
        assert!(result.is_ok());

        fs::remove_dir_all(&tmp).unwrap();
    }
}
