use super::*;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use tokio::io::AsyncWriteExt;

const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Serialize, Deserialize)]
struct DownloadState {
    url: String,
    validator: Option<String>,
    total: Option<u64>,
    completed_sha256: Option<String>,
}

fn io_error(context: &str, source: std::io::Error) -> ProviderError {
    ProviderError::Io {
        context: context.to_string(),
        source,
    }
}

fn unavailable(details: impl Into<String>) -> ProviderError {
    ProviderError::Unavailable {
        details: details.into(),
    }
}

pub(super) fn discard_cached_archive(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("json"));
}

impl OllamaRuntime {
    pub(super) async fn download_archive(
        &self,
        url: &str,
        destination: &Path,
        message: &str,
    ) -> Result<(), ProviderError> {
        for attempt in 0..3 {
            let result = self
                .download_with_idle_timeout(url, destination, message, DOWNLOAD_IDLE_TIMEOUT)
                .await;
            let transient = matches!(&result, Err(ProviderError::Unavailable { details })
                if details.contains("download do runtime Ollama interrompido") || details.contains("download do runtime Ollama incompleto"));
            if !transient || attempt == 2 || self.cancel.load(Ordering::SeqCst) {
                return result;
            }
            self.update_status(|s| {
                s.message = "Conexao interrompida; retomando o download preservado".into()
            })
            .await;
            sleep(Duration::from_millis(500)).await;
        }
        unreachable!("the final download attempt returns its result")
    }

    async fn download_with_idle_timeout(
        &self,
        url: &str,
        destination: &Path,
        message: &str,
        idle: Duration,
    ) -> Result<(), ProviderError> {
        self.ensure_not_cancelled()?;
        let metadata = destination.with_extension("json");
        let mut state = fs::read(&metadata)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DownloadState>(&bytes).ok())
            .filter(|state| state.url == url);
        let mut offset = fs::metadata(destination)
            .map(|m| m.len())
            .unwrap_or_default();
        if let Some(saved) = &state {
            if saved.completed_sha256.is_some()
                && saved.total == Some(offset)
                && saved.completed_sha256.as_deref() == Some(sha256_file(destination)?.as_str())
            {
                return Ok(());
            }
        }
        if state
            .as_ref()
            .is_none_or(|s| s.validator.is_none() || s.total.is_some_and(|n| offset >= n))
        {
            offset = 0;
            state = None;
        }
        self.update_status(|s| {
            s.phase = RuntimePhase::Downloading;
            s.step = "downloading".into();
            s.message = message.into();
            s.can_cancel = true;
            s.progress_percent = None;
            s.bytes_downloaded = offset;
            s.bytes_total = state.as_ref().and_then(|s| s.total);
            s.bytes_per_second = None;
        })
        .await;

        // Large runtime archives are bounded by network inactivity, independently
        // of API/model startup timeouts. A healthy slow connection may run for hours.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(idle)
            .build()
            .map_err(network_error)?;
        let mut request = client.get(url);
        if offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-")).header(
                IF_RANGE,
                state
                    .as_ref()
                    .and_then(|s| s.validator.as_ref())
                    .expect("resume validator"),
            );
        }
        let sending = request.send();
        tokio::pin!(sending);
        let response = loop {
            tokio::select! {
                result = &mut sending => break result.map_err(|e| unavailable(format!("download do runtime Ollama interrompido ao conectar: {e}")))?,
                _ = sleep(Duration::from_millis(100)) => self.ensure_not_cancelled()?,
            }
        };
        if !response.status().is_success() {
            // The cached object may have changed its size. Clear it before a retry.
            if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                discard_cached_archive(destination);
            }
            return Err(unavailable(format!(
                "download do runtime Ollama retornou HTTP {}",
                response.status()
            )));
        }
        let validator = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.starts_with("W/"))
            .or_else(|| {
                response
                    .headers()
                    .get(LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
            })
            .map(ToString::to_string);
        let total = if response.status() == StatusCode::PARTIAL_CONTENT {
            let (start, end, total) = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|s| s.to_str().ok())
                .and_then(parse_content_range)
                .ok_or_else(|| unavailable("Content-Range invalido no download do Ollama"))?;
            if start != offset
                || end < start
                || end >= total
                || (offset > 0 && validator != state.as_ref().and_then(|s| s.validator.clone()))
            {
                discard_cached_archive(destination);
                return Err(unavailable(
                    "O pacote Ollama mudou durante a retomada; tente novamente",
                ));
            }
            Some(total)
        } else {
            // A server ignoring Range or changing If-Range returns a complete body.
            offset = 0;
            response.content_length()
        };
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(offset == 0)
            .append(offset > 0)
            .open(destination)
            .await
            .map_err(|e| io_error("abrindo cache do download Ollama", e))?;
        let mut saved = DownloadState {
            url: url.into(),
            validator,
            total,
            completed_sha256: None,
        };
        write_json_atomic(&metadata, &saved)?;
        let started = Instant::now();
        let mut downloaded = offset;
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                next = stream.next() => next,
                _ = sleep(Duration::from_millis(100)) => { self.ensure_not_cancelled()?; continue; }
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| unavailable(format!("download do runtime Ollama interrompido; os bytes recebidos foram preservados: {e}")))?;
            self.ensure_not_cancelled()?;
            file.write_all(&chunk)
                .await
                .map_err(|e| io_error("gravando cache do download Ollama", e))?;
            downloaded += chunk.len() as u64;
            if total.is_some_and(|n| downloaded > n) {
                discard_cached_archive(destination);
                return Err(unavailable(
                    "download do runtime Ollama excedeu o tamanho anunciado",
                ));
            }
            self.update_status(|s| {
                s.bytes_downloaded = downloaded;
                s.bytes_total = total;
                s.bytes_per_second = Some(
                    ((downloaded - offset) as f64 / started.elapsed().as_secs_f64().max(0.001))
                        as u64,
                );
                s.progress_percent = total
                    .filter(|n| *n > 0)
                    .map(|n| downloaded as f64 / n as f64 * 100.0);
            })
            .await;
        }
        file.flush()
            .await
            .map_err(|e| io_error("finalizando download Ollama", e))?;
        file.sync_all()
            .await
            .map_err(|e| io_error("sincronizando download Ollama", e))?;
        drop(file);
        if total.is_some_and(|n| n != downloaded) {
            return Err(unavailable(
                "download do runtime Ollama incompleto; retome a operacao",
            ));
        }
        saved.total = Some(downloaded);
        saved.completed_sha256 = Some(sha256_file(destination)?);
        write_json_atomic(&metadata, &saved)?;
        Ok(())
    }
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    fn runtime(root: &Path) -> OllamaRuntime {
        OllamaRuntime::new(OllamaRuntimeConfig {
            root: root.to_path_buf(),
            base_url: DEFAULT_MANAGED_URL.into(),
            models_dir: root.join("models"),
            startup_timeout: Duration::from_millis(10),
            auto_install: true,
            auto_start: true,
        })
    }

    async fn request(socket: &mut tokio::net::TcpStream) -> String {
        let mut bytes = vec![0; 4096];
        let len = socket.read(&mut bytes).await.unwrap();
        String::from_utf8(bytes[..len].to_vec())
            .unwrap()
            .to_lowercase()
    }

    #[tokio::test]
    async fn interrupted_download_resumes_across_runtime_instances_and_reuses_completed_archive() {
        let root = std::env::temp_dir().join(format!("mlx-resume-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let dest = root.join("runtime.part");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            assert!(!request(&mut socket).await.contains("range:"));
            socket.write_all(b"HTTP/1.1 200 OK\r\nETag: \"stable\"\r\nContent-Length: 10\r\nConnection: close\r\n\r\nfirst").await.unwrap();
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            let headers = request(&mut socket).await;
            assert!(headers.contains("range: bytes=5-"), "{headers}");
            assert!(headers.contains("if-range: \"stable\""), "{headers}");
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nETag: \"stable\"\r\nContent-Range: bytes 5-9/10\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345").await.unwrap();
        });
        assert!(runtime(&root)
            .download_with_idle_timeout(&url, &dest, "test", DOWNLOAD_IDLE_TIMEOUT)
            .await
            .is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"first");
        runtime(&root)
            .download_archive(&url, &dest, "test")
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"first12345");
        // A third launch uses the complete, hashed archive even with no server.
        runtime(&root)
            .download_archive(&url, &dest, "test")
            .await
            .unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn archive_download_is_independent_of_the_api_total_timeout() {
        let root = std::env::temp_dir().join(format!("mlx-slow-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            for byte in b"abc" {
                sleep(Duration::from_millis(50)).await;
                socket.write_all(&[*byte]).await.unwrap();
            }
        });
        let mut runtime = runtime(&root);
        runtime.client = reqwest::Client::builder()
            .timeout(Duration::from_millis(20))
            .build()
            .unwrap();
        runtime
            .download_with_idle_timeout(
                &url,
                &root.join("runtime.part"),
                "slow",
                Duration::from_millis(100),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(fs::read(root.join("runtime.part")).unwrap(), b"abc");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn stalled_transfer_times_out_with_partial_cache_preserved() {
        let root = std::env::temp_dir().join(format!("mlx-idle-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let dest = root.join("runtime.part");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nETag: \"stable\"\r\nContent-Length: 10\r\nConnection: close\r\n\r\nfirst").await.unwrap();
            sleep(Duration::from_secs(1)).await;
        });
        let result = runtime(&root)
            .download_with_idle_timeout(&url, &dest, "stall", Duration::from_millis(40))
            .await;
        assert!(result.is_err());
        assert_eq!(fs::read(dest).unwrap(), b"first");
        server.abort();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn transient_body_failure_is_retried_automatically_with_a_range_request() {
        let root = std::env::temp_dir().join(format!("mlx-retry-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let dest = root.join("runtime.part");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nETag: \"stable\"\r\nContent-Length: 10\r\nConnection: close\r\n\r\nfirst").await.unwrap();
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            assert!(request(&mut socket).await.contains("range: bytes=5-"));
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nETag: \"stable\"\r\nContent-Range: bytes 5-9/10\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345").await.unwrap();
        });
        runtime(&root)
            .download_archive(&url, &dest, "retry")
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"first12345");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn server_ignoring_range_restarts_the_archive_instead_of_appending() {
        let root = std::env::temp_dir().join(format!("mlx-range-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let dest = root.join("runtime.part");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        fs::write(&dest, b"old").unwrap();
        write_json_atomic(
            &dest.with_extension("json"),
            &DownloadState {
                url: url.clone(),
                validator: Some("\"old\"".into()),
                total: Some(10),
                completed_sha256: None,
            },
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            assert!(request(&mut socket).await.contains("range: bytes=3-"));
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nETag: \"new\"\r\nConnection: close\r\n\r\nnew").await.unwrap();
        });
        runtime(&root)
            .download_archive(&url, &dest, "restart")
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(fs::read(dest).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }
}
