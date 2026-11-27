//! Validate while the upload is still temporary, before any publication effects.

use crate::{
    app::AppState,
    blobs::Received,
    error::{ApiResult, AppError},
};
use spielstand_protocol::VersionMeta;

pub(crate) async fn validate(
    state: &AppState,
    meta: &VersionMeta,
    received: Received,
) -> ApiResult<Received> {
    let permit = state.upload_validation.clone().acquire_owned().await?;
    let files = meta.files.clone();
    let limit = state.settings.max_unpacked;
    tokio::task::spawn_blocking(move || {
        // Own the permit and temporary file until blocking work ends, even if the request
        // is cancelled. Dropping its async future does not stop a decoder already running.
        let _permit = permit;
        spielstand_core::pack::validate(&received.tmp, &files, limit)?;
        Ok::<_, spielstand_core::pack::PackError>(received)
    })
    .await?
    .map_err(|e| AppError::BadRequest(format!("package: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use spielstand_core::pack::{self, Source};
    use spielstand_protocol::Trigger;
    use std::{sync::Arc, time::Duration};

    #[tokio::test]
    async fn validation_waits_for_a_slot_and_releases_it_after_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::open(Settings::for_tests(dir.path().join("server")))
            .await
            .unwrap();
        let input = dir.path().join("save");
        std::fs::write(&input, "hello").unwrap();
        let blob = tempfile::NamedTempFile::new_in(dir.path())
            .unwrap()
            .into_temp_path();
        let packed = pack::pack(
            &[Source {
                path: input,
                portable: "<home>/save".into(),
            }],
            &blob,
        )
        .unwrap();
        let meta = VersionMeta {
            id: "test".into(),
            game: "G".into(),
            parent: None,
            device: "test".into(),
            created_at: chrono::Utc::now(),
            trigger: Trigger::Manual,
            session_seconds: None,
            content_hash: packed.content_hash,
            blob_sha256: packed.blob_sha256.clone(),
            blob_size: packed.blob_size,
            files: vec![],
            prefix: None,
            title: None,
        };
        let received = Received {
            tmp: blob,
            sha256: packed.blob_sha256,
            size: packed.blob_size,
        };
        let permits = Arc::clone(&state.upload_validation)
            .acquire_many_owned(2)
            .await
            .unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task_state = state.clone();
        let mut task = tokio::spawn(async move {
            started.send(()).unwrap();
            validate(&task_state, &meta, received).await
        });
        ready.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut task)
                .await
                .is_err()
        );
        drop(permits);
        assert!(matches!(task.await.unwrap(), Err(AppError::BadRequest(_))));
        assert_eq!(state.upload_validation.available_permits(), 2);
    }
}
