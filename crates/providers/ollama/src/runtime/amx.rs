//! Narrow workaround for the official GCC 11 AMX backend crash on virtualized CPUs.
//! Upstream: https://github.com/ollama/ollama/issues/17249 and PR #17244.
//! Other CPU backends and every GPU backend remain available to Ollama's dispatcher.
use super::*;

const AFFECTED_VERSION: &str = "0.30.7";
const BACKEND_NAME: &str = "libggml-cpu-sapphirerapids.so";
const MARKER_NAME: &str = "cpu-compatibility.json";

#[derive(Debug, Serialize, Deserialize)]
struct CompatibilityMarker {
    version: String,
    executable_sha256: String,
    backend_sha256: String,
    backend_relative_path: String,
    reason: String,
    applied_at_epoch_ms: u128,
}

pub(super) struct RecoveryPlan {
    version_root: PathBuf,
    backend: PathBuf,
    quarantine: PathBuf,
    marker: CompatibilityMarker,
}

pub(super) fn matches_known_failure(
    linux_x64: bool,
    version: Option<&str>,
    origin: &str,
    response: &str,
    log: &str,
) -> bool {
    linux_x64
        && version == Some(AFFECTED_VERSION)
        && origin == "managed"
        && response.contains("segmentation fault")
        && log.contains("AMX model buffer size")
        && log.contains("warming up the model")
}

impl RecoveryPlan {
    pub(super) fn checked(root: &Path, executable: &Path) -> Result<Option<Self>, ProviderError> {
        // Only a previously verified, isolated release may be modified. Neither a
        // system install nor symlinks into a system directory qualify.
        let manifest: ActiveRuntimeManifest = match fs::read(root.join("active.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            Some(manifest) => manifest,
            None => return Ok(None),
        };
        if manifest.version != AFFECTED_VERSION || manifest.executable_relative_path != "bin/ollama"
        {
            return Ok(None);
        }
        let version_root = root.join("versions").join(AFFECTED_VERSION);
        if version_root.join(MARKER_NAME).exists() {
            return Ok(None);
        }
        let expected_executable = version_root.join("bin/ollama");
        let backend = version_root.join("lib/ollama").join(BACKEND_NAME);
        let regular =
            |path: &Path| fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file());
        if !regular(&expected_executable) || !regular(&backend) {
            return Ok(None);
        }
        let Some(canonical_root) = fs::canonicalize(root).ok() else {
            return Ok(None);
        };
        let expected_root = canonical_root.join("versions").join(AFFECTED_VERSION);
        if fs::canonicalize(&version_root).ok().as_ref() != Some(&expected_root)
            || fs::canonicalize(executable).ok().as_ref() != Some(&expected_root.join("bin/ollama"))
            || fs::canonicalize(&backend).ok().as_ref()
                != Some(&expected_root.join("lib/ollama").join(BACKEND_NAME))
            || sha256_file(executable)? != manifest.executable_sha256
        {
            return Ok(None);
        }
        let backend_hash = sha256_file(&backend)?;
        let quarantine = version_root
            .join(".mlx-pilot-cpu-compat")
            .join(BACKEND_NAME);
        if quarantine.exists() {
            return Ok(None);
        }
        Ok(Some(Self {
            version_root,
            backend,
            quarantine,
            marker: CompatibilityMarker {
                version: manifest.version,
                executable_sha256: manifest.executable_sha256,
                backend_sha256: backend_hash,
                backend_relative_path: format!("lib/ollama/{BACKEND_NAME}"),
                reason: "official GCC 11 AMX backend segfault during model warmup; ollama/ollama#17249, fixed upstream by #17244".into(),
                applied_at_epoch_ms: epoch_ms(),
            },
        }))
    }

    pub(super) fn apply(self) -> Result<(), ProviderError> {
        // The caller stops its own Ollama process before moving this one library.
        // Recheck the fingerprint so a concurrent replacement is never quarantined.
        if sha256_file(&self.backend)? != self.marker.backend_sha256 {
            return Err(ProviderError::Unavailable {
                details: "backend AMX foi alterado durante a recuperacao".into(),
            });
        }
        let directory = self.quarantine.parent().expect("quarantine directory");
        fs::create_dir_all(directory).map_err(|source| ProviderError::Io {
            context: format!("criando quarentena AMX {}", directory.display()),
            source,
        })?;
        if fs::symlink_metadata(directory).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(ProviderError::Unavailable {
                details: "diretorio de quarentena AMX nao pode ser um link".into(),
            });
        }
        fs::rename(&self.backend, &self.quarantine).map_err(|source| ProviderError::Io {
            context: "isolando backend AMX defeituoso do runtime gerenciado".into(),
            source,
        })?;
        if let Err(error) = write_json_atomic(&self.version_root.join(MARKER_NAME), &self.marker) {
            let _ = fs::rename(&self.quarantine, &self.backend);
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("mlx-amx-{}", Uuid::new_v4()));
        let version = root.join("versions").join(AFFECTED_VERSION);
        fs::create_dir_all(version.join("bin")).unwrap();
        fs::create_dir_all(version.join("lib/ollama/cuda_v13")).unwrap();
        let executable = version.join("bin/ollama");
        fs::write(&executable, b"verified executable").unwrap();
        fs::write(
            version.join("lib/ollama").join(BACKEND_NAME),
            b"known-bad amx",
        )
        .unwrap();
        fs::write(version.join("lib/ollama/libggml-cpu-haswell.so"), b"avx2").unwrap();
        fs::write(version.join("lib/ollama/cuda_v13/libggml-cuda.so"), b"cuda").unwrap();
        let manifest = ActiveRuntimeManifest {
            schema_version: 1,
            version: AFFECTED_VERSION.into(),
            executable_relative_path: "bin/ollama".into(),
            executable_sha256: sha256_file(&executable).unwrap(),
            source_url: "official verified archive".into(),
            installed_at_epoch_ms: epoch_ms(),
            validated_at_epoch_ms: epoch_ms(),
        };
        write_json_atomic(&root.join("active.json"), &manifest).unwrap();
        (root, executable)
    }

    #[test]
    fn only_the_known_managed_linux_amx_segfault_matches() {
        let response =
            "llama-server process has terminated: signal: segmentation fault (core dumped)";
        let log = "AMX model buffer size = 414.51 MiB\nwarming up the model with an empty run";
        assert!(matches_known_failure(
            true,
            Some(AFFECTED_VERSION),
            "managed",
            response,
            log
        ));
        for (platform, version, origin, detail, evidence) in [
            (false, Some(AFFECTED_VERSION), "managed", response, log),
            (true, Some("0.31.0"), "managed", response, log),
            (true, Some(AFFECTED_VERSION), "system", response, log),
            (true, Some(AFFECTED_VERSION), "external", response, log),
            (
                true,
                Some(AFFECTED_VERSION),
                "managed",
                "out of memory",
                log,
            ),
            (
                true,
                Some(AFFECTED_VERSION),
                "managed",
                response,
                "CPU model buffer size",
            ),
        ] {
            assert!(!matches_known_failure(
                platform, version, origin, detail, evidence
            ));
        }
    }

    #[test]
    fn quarantine_is_persistent_preserves_other_backends_and_install_manifest() {
        let (root, executable) = fixture();
        let active_before = fs::read(root.join("active.json")).unwrap();
        let version = root.join("versions").join(AFFECTED_VERSION);
        RecoveryPlan::checked(&root, &executable)
            .unwrap()
            .unwrap()
            .apply()
            .unwrap();
        assert!(!version.join("lib/ollama").join(BACKEND_NAME).exists());
        assert_eq!(
            fs::read(version.join(".mlx-pilot-cpu-compat").join(BACKEND_NAME)).unwrap(),
            b"known-bad amx"
        );
        assert_eq!(
            fs::read(version.join("lib/ollama/libggml-cpu-haswell.so")).unwrap(),
            b"avx2"
        );
        assert_eq!(
            fs::read(version.join("lib/ollama/cuda_v13/libggml-cuda.so")).unwrap(),
            b"cuda"
        );
        assert_eq!(fs::read(root.join("active.json")).unwrap(), active_before);
        assert_eq!(fs::read(&executable).unwrap(), b"verified executable");
        let marker: CompatibilityMarker =
            serde_json::from_slice(&fs::read(version.join(MARKER_NAME)).unwrap()).unwrap();
        assert_eq!(marker.backend_sha256.len(), 64);
        // Reopening or another failed model cannot apply the workaround twice.
        assert!(RecoveryPlan::checked(&root, &executable).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_manifest_external_executable_and_modified_executable_are_rejected() {
        let (root, executable) = fixture();
        let external = root.join("system-ollama");
        fs::copy(&executable, &external).unwrap();
        assert!(RecoveryPlan::checked(&root, &external).unwrap().is_none());
        fs::write(&executable, b"modified executable").unwrap();
        assert!(RecoveryPlan::checked(&root, &executable).unwrap().is_none());
        fs::write(root.join("active.json"), b"invalid manifest").unwrap();
        assert!(RecoveryPlan::checked(&root, &executable).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn backend_or_library_directory_symlinks_cannot_modify_external_files() {
        use std::os::unix::fs::symlink;
        let (root, executable) = fixture();
        let version = root.join("versions").join(AFFECTED_VERSION);
        let external = root.join("external-amx.so");
        fs::write(&external, b"system backend").unwrap();
        let backend = version.join("lib/ollama").join(BACKEND_NAME);
        fs::remove_file(&backend).unwrap();
        symlink(&external, &backend).unwrap();
        assert!(RecoveryPlan::checked(&root, &executable).unwrap().is_none());
        fs::remove_file(&backend).unwrap();
        let external_lib = root.join("external-lib");
        fs::rename(version.join("lib/ollama"), &external_lib).unwrap();
        fs::write(external_lib.join(BACKEND_NAME), b"system backend").unwrap();
        symlink(&external_lib, version.join("lib/ollama")).unwrap();
        assert!(RecoveryPlan::checked(&root, &executable).unwrap().is_none());
        assert_eq!(fs::read(&external).unwrap(), b"system backend");
        fs::remove_dir_all(root).unwrap();
    }
}
