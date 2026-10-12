use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{
    manifest::PatchManifest,
    patch::Patch,
    update::{UpdateQuery, UpdateResponse},
};

fn is_supported_channel(channel: &str) -> bool {
    matches!(channel, "development" | "staging" | "beta" | "stable")
}

fn is_rollout_eligible(
    app_id: Uuid,
    patch_id: Uuid,
    device_id: Option<&str>,
    percentage: i32,
) -> bool {
    match percentage {
        100 => true,
        0 => false,
        _ => {
            let Some(device_id) = device_id.filter(|value| !value.is_empty()) else {
                return false;
            };
            let digest = Sha256::digest(format!("{app_id}:{patch_id}:{device_id}"));
            let bucket = u16::from_be_bytes([digest[0], digest[1]]) as u32 * 100 / 65_536;
            bucket < percentage as u32
        }
    }
}

pub async fn check_update(
    Path(app_id): Path<Uuid>,
    Query(query): Query<UpdateQuery>,
    State(pool): State<PgPool>,
) -> Result<Json<UpdateResponse>, StatusCode> {
    let current_patch = query.patch.unwrap_or(0);

    if !is_supported_channel(&query.channel)
        || query
            .device_id
            .as_ref()
            .is_some_and(|value| value.len() > 200)
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let patch = sqlx::query_as::<_, Patch>(
        r#"
            SELECT
                p.id,
                p.release_id,

                p.patch_number,
                p.manifest_version,
                p.rollout_percentage,

                p.file_path,
                p.sha256,
                p.file_size,

                p.signature,
                p.key_id,

                p.patch_format,
                p.architecture,

                p.base_sha256,
                p.base_patch_number,
                p.target_sha256,
                p.target_file_size,

                p.created_at

            FROM patches p

            JOIN releases r
                ON p.release_id = r.id

            JOIN signing_keys sk
                ON p.key_id = sk.key_id AND sk.status <> 'revoked'

            WHERE
                r.app_id = $1
                AND r.version = $2
                AND r.platform = $3
                AND r.channel = $4
                AND p.base_patch_number = $5
                AND ($6::VARCHAR IS NULL OR p.architecture = $6)

            ORDER BY
                p.patch_number DESC,
                p.architecture ASC

            LIMIT 1
            "#,
    )
    .bind(app_id)
    .bind(&query.version)
    .bind(&query.platform)
    .bind(&query.channel)
    .bind(current_patch)
    .bind(query.architecture.as_deref())
    .fetch_optional(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to check update: {error}");

        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match patch {
        Some(patch)
            if is_rollout_eligible(
                app_id,
                patch.id,
                query.device_id.as_deref(),
                patch.rollout_percentage,
            ) =>
        {
            let signature = patch.signature.ok_or_else(|| {
                eprintln!("Patch {} has no signature", patch.patch_number);

                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let key_id = patch.key_id.ok_or_else(|| {
                eprintln!("Patch {} has no key_id", patch.patch_number);

                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let manifest = PatchManifest {
                manifest_version: patch.manifest_version,

                key_id,

                app_id,

                release_id: patch.release_id,

                version: query.version.clone(),

                platform: query.platform.clone(),

                channel: query.channel.clone(),

                patch_number: patch.patch_number,

                artifact_path: patch.file_path,

                sha256: patch.sha256,

                file_size: patch.file_size,

                patch_format: patch.patch_format,

                architecture: patch.architecture,

                base_sha256: patch.base_sha256,

                base_patch_number: patch.base_patch_number,

                target_sha256: patch.target_sha256,

                target_file_size: patch.target_file_size,

                rollout_percentage: patch.rollout_percentage,

                signature,
            };

            Ok(Json(UpdateResponse {
                update_available: true,
                manifest: Some(manifest),
            }))
        }

        _ => Ok(Json(UpdateResponse {
            update_available: false,
            manifest: None,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::is_rollout_eligible;
    use uuid::Uuid;

    #[test]
    fn rollout_boundaries_are_enforced() {
        let app = Uuid::nil();
        let patch = Uuid::from_u128(1);
        assert!(is_rollout_eligible(app, patch, None, 100));
        assert!(!is_rollout_eligible(app, patch, Some("device"), 0));
        assert!(!is_rollout_eligible(app, patch, None, 50));
    }

    #[test]
    fn rollout_assignment_is_deterministic() {
        let app = Uuid::nil();
        let patch = Uuid::from_u128(2);
        let first = is_rollout_eligible(app, patch, Some("device-123"), 42);
        assert_eq!(
            first,
            is_rollout_eligible(app, patch, Some("device-123"), 42)
        );
    }
}
