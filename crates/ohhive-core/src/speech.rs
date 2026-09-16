//! One-pass speech execution for community cards. No text draft/critique loop or caller paths.
use crate::{
    backend::{collect, Backend, Completion},
    capability::{Modality, Requirements},
    job::{Job, JobKind},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::watch;
use uuid::Uuid;

pub const MAX_AUDIO_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechInput {
    pub version: u8,
    pub artifact_hash: String,
    pub bytes: u64,
    pub mime: String,
    pub language: Option<String>,
}
impl SpeechInput {
    pub fn validate(&self) -> Result<(), SpeechError> {
        if self.version != 1
            || self.bytes == 0
            || self.bytes > MAX_AUDIO_BYTES
            || self.artifact_hash.len() != 64
            || !self
                .artifact_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !matches!(
                self.mime.as_str(),
                "audio/wav"
                    | "audio/x-wav"
                    | "audio/mpeg"
                    | "audio/mp4"
                    | "audio/flac"
                    | "audio/ogg"
            )
            || self.language.as_ref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 16
                    || !s.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
            })
        {
            return Err(SpeechError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpeechError {
    #[error("Invalid speech input manifest")]
    InvalidInput,
    #[error("Audio content does not match its manifest")]
    ContentMismatch,
    #[error("Cannot stage transcription audio")]
    Staging,
    #[error("Transcription cancelled")]
    Cancelled,
    #[error("Transcription deadline reached")]
    Deadline,
    #[error("Speech backend failed")]
    Backend,
    #[error("Transcript is incomplete or too large")]
    InvalidOutput,
}

struct StagedAudio(PathBuf);
impl Drop for StagedAudio {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
impl StagedAudio {
    fn new(root: &Path, bytes: &[u8], mime: &str) -> Result<(Self, PathBuf), SpeechError> {
        let dir = root.join(format!("speech-{}", Uuid::new_v4()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir).map_err(|_| SpeechError::Staging)?;
        let guard = Self(dir);
        let extension = match mime {
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/mpeg" => "mp3",
            "audio/mp4" => "m4a",
            "audio/flac" => "flac",
            "audio/ogg" => "ogg",
            _ => return Err(SpeechError::InvalidInput),
        };
        let file = guard.0.join(format!("input.{extension}"));
        std::fs::write(&file, bytes).map_err(|_| SpeechError::Staging)?;
        Ok((guard, file))
    }
}

/// Caller owns artifact retrieval and authenticated lease/receipt persistence. This function never
/// sends credentials, selects a worker, prices work or accepts a remote-supplied filesystem path.
/// Dropping this future cancels the model request and removes its staged input.
// Explicit input, job identity and lifetime controls keep this executor independent of hub authority.
#[allow(clippy::too_many_arguments)]
pub async fn transcribe(
    backend: &dyn Backend,
    manifest: &SpeechInput,
    audio: &[u8],
    project_id: Uuid,
    card_id: Uuid,
    scratch_root: &Path,
    deadline: Duration,
    mut cancel: watch::Receiver<bool>,
) -> Result<Completion, SpeechError> {
    manifest.validate()?;
    if *cancel.borrow() {
        return Err(SpeechError::Cancelled);
    }
    if deadline.is_zero() {
        return Err(SpeechError::Deadline);
    }
    if audio.len() as u64 != manifest.bytes
        || format!("{:x}", Sha256::digest(audio)) != manifest.artifact_hash
    {
        return Err(SpeechError::ContentMismatch);
    }
    let (_staged, path) = StagedAudio::new(scratch_root, audio, &manifest.mime)?;
    let job = Job {
        id: Uuid::new_v4(),
        kind: JobKind::Inference,
        project_id,
        card_id: Some(card_id),
        parent: None,
        requirements: Requirements {
            modality: Some(Modality::Speech),
            ..Default::default()
        },
        input: serde_json::json!({"audio_path": path, "language": manifest.language}),
        resume_from: None,
        created_at: chrono::Utc::now(),
    };
    let run = async {
        let stream = backend.run(&job).await.map_err(|_| SpeechError::Backend)?;
        // Bound output while reading, not only after accumulating a response.
        use futures::StreamExt;
        let mut stream = stream;
        let mut chunks = Vec::new();
        let mut bytes = 0usize;
        let mut finished = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| SpeechError::Backend)?;
            bytes = bytes.saturating_add(chunk.text.len());
            if bytes > MAX_TRANSCRIPT_BYTES || chunks.len() >= 65536 {
                return Err(SpeechError::InvalidOutput);
            }
            let done = chunk.done;
            chunks.push(Ok(chunk));
            if done {
                finished = true;
                break;
            }
        }
        if !finished {
            return Err(SpeechError::InvalidOutput);
        }
        let completion = collect(Box::pin(futures::stream::iter(chunks)))
            .await
            .map_err(|_| SpeechError::Backend)?;
        if completion.truncated {
            return Err(SpeechError::InvalidOutput);
        }
        Ok(completion)
    };
    tokio::select! {
        biased;
        _ = async { loop { if *cancel.borrow() || cancel.changed().await.is_err() { break; } } } => Err(SpeechError::Cancelled),
        _ = tokio::time::sleep(deadline) => Err(SpeechError::Deadline),
        result = run => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::{BackendError, Chunk, ChunkStream},
        capability::Capabilities,
        ledger::Usage,
    };
    struct TestBackend {
        mode: u8,
    }
    #[async_trait::async_trait]
    impl Backend for TestBackend {
        fn name(&self) -> &'static str {
            "test-speech"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn capabilities(&self) -> Result<Capabilities, BackendError> {
            unreachable!()
        }
        async fn run<'a>(&'a self, job: &'a Job) -> Result<ChunkStream<'a>, BackendError> {
            assert_eq!(job.requirements.modality, Some(Modality::Speech));
            assert_eq!(job.kind, JobKind::Inference);
            let path = job.input["audio_path"].as_str().unwrap();
            assert_eq!(std::fs::read(path).unwrap(), b"fixture audio");
            assert!(path.ends_with("input.wav"));
            if self.mode == 1 {
                std::future::pending::<()>().await;
            }
            let mut chunks = vec![Ok(Chunk::text(if self.mode == 4 {
                "x".repeat(MAX_TRANSCRIPT_BYTES + 1)
            } else {
                "Hello world".into()
            }))];
            if self.mode != 3 {
                chunks.push(Ok(if self.mode == 2 {
                    Chunk::done_truncated(Usage::default())
                } else {
                    Chunk::done(Usage {
                        compute_seconds: 1.25,
                        ..Default::default()
                    })
                }));
            }
            Ok(Box::pin(futures::stream::iter(chunks)))
        }
    }
    fn manifest() -> SpeechInput {
        SpeechInput {
            version: 1,
            artifact_hash: format!("{:x}", Sha256::digest(b"fixture audio")),
            bytes: 13,
            mime: "audio/wav".into(),
            language: Some("en".into()),
        }
    }
    fn root() -> StagedAudio {
        let path = std::env::temp_dir().join(format!("hive-speech-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        StagedAudio(path)
    }
    #[tokio::test]
    async fn single_pass_preserves_usage_and_cleans_up() {
        let root = root();
        let (_tx, rx) = watch::channel(false);
        let result = transcribe(
            &TestBackend { mode: 0 },
            &manifest(),
            b"fixture audio",
            Uuid::new_v4(),
            Uuid::new_v4(),
            &root.0,
            Duration::from_secs(1),
            rx,
        )
        .await
        .unwrap();
        assert_eq!(result.text, "Hello world");
        assert_eq!(result.usage.compute_seconds, 1.25);
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
    }
    #[tokio::test]
    async fn bad_content_and_output_never_survive() {
        let root = root();
        let (_tx, rx) = watch::channel(false);
        let backend = TestBackend { mode: 0 };
        assert!(matches!(
            transcribe(
                &backend,
                &manifest(),
                b"wrong",
                Uuid::new_v4(),
                Uuid::new_v4(),
                &root.0,
                Duration::from_secs(1),
                rx.clone()
            )
            .await,
            Err(SpeechError::ContentMismatch)
        ));
        let mut wrong_hash = manifest();
        wrong_hash.artifact_hash = "0".repeat(64);
        assert!(matches!(
            transcribe(
                &backend,
                &wrong_hash,
                b"fixture audio",
                Uuid::new_v4(),
                Uuid::new_v4(),
                &root.0,
                Duration::from_secs(1),
                rx.clone()
            )
            .await,
            Err(SpeechError::ContentMismatch)
        ));
        for mode in [2, 3, 4] {
            assert!(matches!(
                transcribe(
                    &TestBackend { mode },
                    &manifest(),
                    b"fixture audio",
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    &root.0,
                    Duration::from_secs(1),
                    rx.clone()
                )
                .await,
                Err(SpeechError::InvalidOutput)
            ));
            assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
        }
    }
    #[tokio::test]
    async fn timeout_cancel_and_dropped_future_remove_audio() {
        let root = root();
        let (tx, rx) = watch::channel(false);
        let backend = TestBackend { mode: 1 };
        let manifest = manifest();
        assert!(matches!(
            transcribe(
                &backend,
                &manifest,
                b"fixture audio",
                Uuid::new_v4(),
                Uuid::new_v4(),
                &root.0,
                Duration::from_millis(10),
                rx.clone()
            )
            .await,
            Err(SpeechError::Deadline)
        ));
        let future = transcribe(
            &backend,
            &manifest,
            b"fixture audio",
            Uuid::new_v4(),
            Uuid::new_v4(),
            &root.0,
            Duration::from_secs(1),
            rx.clone(),
        );
        let signal = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            tx.send(true).unwrap();
        };
        let (result, _) = tokio::join!(future, signal);
        assert!(matches!(result, Err(SpeechError::Cancelled)));
        tx.send(false).unwrap();
        let future = transcribe(
            &backend,
            &manifest,
            b"fixture audio",
            Uuid::new_v4(),
            Uuid::new_v4(),
            &root.0,
            Duration::from_secs(1),
            rx,
        );
        assert!(tokio::time::timeout(Duration::from_millis(10), future)
            .await
            .is_err());
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
    }
    #[tokio::test]
    async fn whisper_http_adapter_receives_staged_audio_and_returns_transcript() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buf = [0u8; 1024];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                assert!(request.len() < 16384);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = header
                        .lines()
                        .find_map(|s| s.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(text.starts_with("POST /inference "));
            assert!(text.contains("fixture audio"));
            assert!(text.contains("input.wav"));
            assert!(!text.to_lowercase().contains("authorization:"));
            let body = r#"{"text":"A community transcript"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let root = root();
        let (_tx, rx) = watch::channel(false);
        let backend = crate::backend::whisper::WhisperCppBackend::new(endpoint, "test-model");
        let result = transcribe(
            &backend,
            &manifest(),
            b"fixture audio",
            Uuid::new_v4(),
            Uuid::new_v4(),
            &root.0,
            Duration::from_secs(2),
            rx,
        )
        .await
        .unwrap();
        assert_eq!(result.text, "A community transcript");
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
        server.await.unwrap();
    }

    #[test]
    fn manifests_cannot_inject_paths_or_unbounded_inputs() {
        assert!(manifest().validate().is_ok());
        for value in [
            "../audio.wav",
            "https://host/audio",
            "A".repeat(64).as_str(),
        ] {
            let mut m = manifest();
            m.artifact_hash = value.into();
            assert!(m.validate().is_err());
        }
        let mut m = manifest();
        m.bytes = MAX_AUDIO_BYTES + 1;
        assert!(m.validate().is_err());
        let mut json = serde_json::to_value(manifest()).unwrap();
        json["audio_path"] = "/etc/passwd".into();
        assert!(serde_json::from_value::<SpeechInput>(json).is_err());
    }
}
