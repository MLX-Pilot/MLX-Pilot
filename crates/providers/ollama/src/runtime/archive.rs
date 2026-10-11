//! Official release selection and portable extraction for the managed runtime.
use super::{extract_zip, ProviderError};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
};

pub(super) fn release_asset(os: &str, arch: &str) -> Result<&'static str, ProviderError> {
    let asset = match (os, arch) {
        ("macos", "aarch64" | "x86_64") => "ollama-darwin.tgz",
        ("windows", "x86_64") => "ollama-windows-amd64.zip",
        ("windows", "aarch64") => "ollama-windows-arm64.zip",
        ("linux", "x86_64") => "ollama-linux-amd64.tar.zst",
        ("linux", "aarch64") => "ollama-linux-arm64.tar.zst",
        _ => {
            return Err(invalid(format!(
                "plataforma Ollama nao suportada: {os}/{arch}"
            )))
        }
    };
    Ok(asset)
}
fn invalid(message: impl ToString) -> ProviderError {
    ProviderError::Unavailable {
        details: message.to_string(),
    }
}

pub(super) fn extract(path: &Path, destination: &Path) -> Result<(), ProviderError> {
    let mut magic = [0u8; 4];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|e| invalid(format!("arquivo Ollama invalido: {e}")))?;
    if magic.starts_with(b"PK") {
        return extract_zip(path, destination);
    }
    let file = File::open(path).map_err(|e| invalid(e.to_string()))?;
    let reader: Box<dyn Read> = if magic.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(file))
    } else if magic == [0x28, 0xb5, 0x2f, 0xfd] {
        Box::new(zstd::stream::read::Decoder::new(file).map_err(|e| invalid(e.to_string()))?)
    } else {
        return Err(invalid("formato do pacote Ollama nao suportado"));
    };
    std::fs::create_dir_all(destination).map_err(|e| invalid(e.to_string()))?;
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(|e| invalid(e.to_string()))? {
        let mut entry = entry.map_err(|e| invalid(e.to_string()))?;
        let path = entry.path().map_err(|e| invalid(e.to_string()))?;
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(invalid("pacote Ollama contem caminho inseguro"));
        }
        if let Some(link) = entry.link_name().map_err(|e| invalid(e.to_string()))? {
            // Official runtime library links are local. Never grant an archive an escape path.
            if link
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
            {
                return Err(invalid("pacote Ollama contem link inseguro"));
            }
        }
        if !entry
            .unpack_in(destination)
            .map_err(|e| invalid(e.to_string()))?
        {
            return Err(invalid("pacote Ollama contem caminho inseguro"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};
    #[test]
    fn selects_arch_specific_official_assets() {
        assert_eq!(
            release_asset("macos", "aarch64").unwrap(),
            "ollama-darwin.tgz"
        );
        assert_eq!(
            release_asset("windows", "aarch64").unwrap(),
            "ollama-windows-arm64.zip"
        );
        assert_eq!(
            release_asset("linux", "x86_64").unwrap(),
            "ollama-linux-amd64.tar.zst"
        );
        assert!(release_asset("linux", "riscv64").is_err());
    }
    fn tar_bytes(link: bool) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o755);
        if link {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_link_name("../../escape").unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "ollama", std::io::empty())
                .unwrap();
        } else {
            header.set_size(4);
            header.set_cksum();
            builder
                .append_data(&mut header, "ollama", &b"test"[..])
                .unwrap();
        }
        builder.into_inner().unwrap()
    }
    #[test]
    fn extracts_gzip_zstd_and_preserves_executable_mode() {
        let root = std::env::temp_dir().join(format!("mlx-archives-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&tar_bytes(false)).unwrap();
        for (name, bytes) in [
            ("gzip", gzip.finish().unwrap()),
            (
                "zstd",
                zstd::stream::encode_all(&tar_bytes(false)[..], 0).unwrap(),
            ),
        ] {
            let path = root.join(name);
            fs::write(&path, bytes).unwrap();
            let out = root.join(format!("{name}-out"));
            extract(&path, &out).unwrap();
            assert_eq!(fs::read(out.join("ollama")).unwrap(), b"test");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_ne!(
                    fs::metadata(out.join("ollama"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o111,
                    0
                );
            }
        }
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn rejects_tar_links_outside_destination() {
        let root = std::env::temp_dir().join(format!("mlx-links-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let bytes = zstd::stream::encode_all(&tar_bytes(true)[..], 0).unwrap();
        let path = root.join("bad.tar.zst");
        fs::write(&path, bytes).unwrap();
        assert!(extract(&path, &root.join("out")).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
