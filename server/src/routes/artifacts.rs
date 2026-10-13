use axum::{
    Json,
    body::Body,
    extract::{Extension, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::ErrorKind, path::PathBuf};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};
use uuid::Uuid;

const SHA256_HEADER: &str = "x-patchforge-sha256";
const FILE_SIZE_HEADER: &str = "x-patchforge-file-size";

#[derive(Clone)]
pub struct ArtifactStorage {
    root: PathBuf,
    max_size: u64,
}

#[derive(Debug, Serialize)]
pub struct UploadedArtifact {
    artifact_path: String,
    sha256: String,
    file_size: u64,
    created: bool,
}

#[derive(Debug)]
pub(crate) enum UploadError {
    BadRequest(&'static str),
    Conflict(&'static str),
    TooLarge(u64),
    Io(std::io::Error),
}

impl ArtifactStorage {
    pub async fn new(root: PathBuf, max_size: u64) -> std::io::Result<Self> {
        if max_size == 0 {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "artifact size limit must be greater than zero",
            ));
        }
        fs::create_dir_all(root.join("patches")).await?;
        Ok(Self { root, max_size })
    }

    pub(crate) fn provider_name(&self) -> &'static str {
        "local"
    }

    pub(crate) fn max_size(&self) -> u64 {
        self.max_size
    }

    fn destination(&self, artifact_path: &str) -> Result<PathBuf, UploadError> {
        let Some(filename) = artifact_path.strip_prefix("patches/") else {
            return Err(UploadError::BadRequest(
                "Artifact path must start with patches/",
            ));
        };
        if filename.is_empty()
            || filename.starts_with('.')
            || !filename.ends_with(".patch")
            || !filename
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(UploadError::BadRequest("Invalid patch artifact filename"));
        }
        Ok(self.root.join("patches").join(filename))
    }

    pub(crate) async fn verifies(
        &self,
        artifact_path: &str,
        expected_sha256: &str,
        expected_size: u64,
    ) -> Result<bool, std::io::Error> {
        let Ok(destination) = self.destination(artifact_path) else {
            return Ok(false);
        };
        let metadata = match fs::symlink_metadata(&destination).await {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if metadata.len() != expected_size {
            return Ok(false);
        }
        let (actual_sha256, actual_size) =
            file_digest(&destination)
                .await
                .map_err(|error| match error {
                    UploadError::Io(error) => error,
                    _ => std::io::Error::other("failed to verify stored artifact"),
                })?;
        Ok(actual_size == expected_size && actual_sha256.eq_ignore_ascii_case(expected_sha256))
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "invalid_artifact", message),
            Self::Conflict(message) => (StatusCode::CONFLICT, "artifact_conflict", message),
            Self::TooLarge(limit) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "artifact_too_large",
                if limit == 0 {
                    "Artifact exceeds the configured size limit"
                } else {
                    "Declared artifact size exceeds the configured size limit"
                },
            ),
            Self::Io(error) => {
                eprintln!("Artifact storage error: {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "artifact_storage_failed",
                    "Failed to store patch artifact",
                )
            }
        };
        (status, Json(json!({ "error": error, "message": message }))).into_response()
    }
}

fn required_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str, UploadError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or(UploadError::BadRequest(
            "Required artifact metadata header is missing",
        ))
}

async fn file_digest(path: &PathBuf) -> Result<(String, u64), UploadError> {
    let bytes = fs::read(path).await.map_err(UploadError::Io)?;
    let size = u64::try_from(bytes.len())
        .map_err(|_| UploadError::BadRequest("Artifact size is invalid"))?;
    Ok((hex::encode(Sha256::digest(&bytes)), size))
}

async fn existing_artifact(
    destination: &PathBuf,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<Option<UploadedArtifact>, UploadError> {
    match fs::symlink_metadata(destination).await {
        Ok(metadata) if metadata.file_type().is_file() => {
            let (sha256, file_size) = file_digest(destination).await?;
            if sha256 != expected_sha256 || file_size != expected_size {
                return Err(UploadError::Conflict(
                    "An artifact already exists at this path with different content",
                ));
            }
            Ok(Some(UploadedArtifact {
                artifact_path: format!(
                    "patches/{}",
                    destination
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                ),
                sha256,
                file_size,
                created: false,
            }))
        }
        Ok(_) => Err(UploadError::Conflict(
            "Artifact destination exists and is not a regular file",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(UploadError::Io(error)),
    }
}

pub async fn upload_artifact(
    Extension(storage): Extension<ArtifactStorage>,
    Path(artifact_path): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<UploadedArtifact>), UploadError> {
    let destination = storage.destination(&artifact_path)?;
    let expected_sha256 = required_header(&headers, SHA256_HEADER)?.to_ascii_lowercase();
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(UploadError::BadRequest("Invalid artifact SHA-256 header"));
    }
    let expected_size = required_header(&headers, FILE_SIZE_HEADER)?
        .parse::<u64>()
        .map_err(|_| UploadError::BadRequest("Invalid artifact file-size header"))?;
    if expected_size > storage.max_size {
        return Err(UploadError::TooLarge(storage.max_size));
    }

    if let Some(existing) = existing_artifact(&destination, &expected_sha256, expected_size).await?
    {
        return Ok((StatusCode::OK, Json(existing)));
    }

    let filename = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(UploadError::BadRequest("Invalid artifact filename"))?;
    let temporary = destination.with_file_name(format!(".{filename}.{}.upload", Uuid::new_v4()));
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await
        .map_err(UploadError::Io)?;
    let mut stream = body.into_data_stream();
    let mut hasher = Sha256::new();
    let mut received = 0_u64;

    let write_result: Result<(), UploadError> = async {
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|_| UploadError::BadRequest("Failed to read artifact body"))?;
            received = received
                .checked_add(u64::try_from(chunk.len()).map_err(|_| UploadError::TooLarge(0))?)
                .ok_or(UploadError::TooLarge(0))?;
            if received > storage.max_size || received > expected_size {
                return Err(UploadError::TooLarge(0));
            }
            hasher.update(&chunk);
            output.write_all(&chunk).await.map_err(UploadError::Io)?;
        }
        output.flush().await.map_err(UploadError::Io)?;
        output.sync_all().await.map_err(UploadError::Io)?;
        Ok(())
    }
    .await;

    if let Err(error) = write_result {
        drop(output);
        let _ = fs::remove_file(&temporary).await;
        return Err(error);
    }
    drop(output);

    let actual_sha256 = hex::encode(hasher.finalize());
    if received != expected_size || actual_sha256 != expected_sha256 {
        let _ = fs::remove_file(&temporary).await;
        return Err(UploadError::BadRequest(
            "Artifact body does not match its declared SHA-256 and size",
        ));
    }

    if let Err(error) = fs::hard_link(&temporary, &destination).await {
        let _ = fs::remove_file(&temporary).await;
        if error.kind() == ErrorKind::AlreadyExists
            && let Some(existing) =
                existing_artifact(&destination, &expected_sha256, expected_size).await?
        {
            return Ok((StatusCode::OK, Json(existing)));
        }
        return Err(UploadError::Io(error));
    }
    fs::remove_file(&temporary).await.map_err(UploadError::Io)?;

    Ok((
        StatusCode::CREATED,
        Json(UploadedArtifact {
            artifact_path,
            sha256: actual_sha256,
            file_size: received,
            created: true,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::{ArtifactStorage, upload_artifact};
    use axum::{body::Body, extract::Extension, extract::Path, http::HeaderMap};
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    fn headers(bytes: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-patchforge-sha256",
            hex::encode(Sha256::digest(bytes)).parse().unwrap(),
        );
        headers.insert(
            "x-patchforge-file-size",
            bytes.len().to_string().parse().unwrap(),
        );
        headers
    }

    #[tokio::test]
    async fn stores_verified_patch_and_accepts_an_identical_retry() {
        let directory = TempDir::new().unwrap();
        let storage = ArtifactStorage::new(directory.path().to_path_buf(), 1024)
            .await
            .unwrap();
        let bytes = b"BSDIFF40-patch";
        let path = "patches/release-arm64-v8a-patch-1-deadbeef.patch".to_string();

        let (first_status, first) = upload_artifact(
            Extension(storage.clone()),
            Path(path.clone()),
            headers(bytes),
            Body::from(bytes.as_slice()),
        )
        .await
        .unwrap();
        assert_eq!(first_status, axum::http::StatusCode::CREATED);
        assert!(first.0.created);
        assert!(
            storage
                .verifies(
                    &path,
                    &hex::encode(Sha256::digest(bytes)),
                    bytes.len() as u64
                )
                .await
                .unwrap()
        );

        let (retry_status, retry) = upload_artifact(
            Extension(storage),
            Path(path),
            headers(bytes),
            Body::from(bytes.as_slice()),
        )
        .await
        .unwrap();
        assert_eq!(retry_status, axum::http::StatusCode::OK);
        assert!(!retry.0.created);
    }

    #[tokio::test]
    async fn rejects_unsafe_paths_and_mismatched_content() {
        let directory = TempDir::new().unwrap();
        let storage = ArtifactStorage::new(directory.path().to_path_buf(), 1024)
            .await
            .unwrap();
        let bytes = b"valid";

        assert!(
            upload_artifact(
                Extension(storage.clone()),
                Path("patches/../escape.patch".to_string()),
                headers(bytes),
                Body::from(bytes.as_slice()),
            )
            .await
            .is_err()
        );

        assert!(
            upload_artifact(
                Extension(storage),
                Path("patches/safe.patch".to_string()),
                headers(bytes),
                Body::from("different"),
            )
            .await
            .is_err()
        );
    }
}
