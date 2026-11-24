//! Product image upload and delivery.
//!
//! Ported from `handlers_images.go`. The upload accepts one `image` part of at
//! most 5 MiB, sniffs PNG/JPEG/WebP from the bytes and stores the file in the
//! same transaction as its audit row. The download route serves the stored
//! bytes with a long-lived immutable cache header.

use axum::body::Body;
use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use azahar_ids::new as new_id;
use azahar_store::{AuditEntry, StoreError};
use chrono::Utc;
use serde_json::json;

use crate::middleware::{CurrentUser, internal_error};
use crate::respond::{ApiProblem, json_response, problem};
use crate::state::AppState;

/// The Go `maxProductImageBytes`: 5 MiB.
pub const MAX_PRODUCT_IMAGE_BYTES: usize = 5 << 20;
/// The Go body limit: the image limit plus 1 MiB of multipart framing.
pub const MAX_UPLOAD_BODY_BYTES: usize = MAX_PRODUCT_IMAGE_BYTES + (1 << 20);

fn upload_too_large() -> ApiProblem {
    problem(
        413,
        "payload_too_large",
        "The image is too large. Use 5 MB or less.",
    )
}

/// `http.DetectContentType` for the three accepted image formats.
#[must_use]
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    let head = bytes.get(..512).unwrap_or(bytes);
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    // Go's masked signature is `RIFF????WEBPVP`, fourteen bytes.
    if head.len() >= 14
        && &head[..4] == b"RIFF"
        && &head[8..12] == b"WEBP"
        && &head[12..14] == b"VP"
    {
        return Some("image/webp");
    }
    None
}

/// `POST /api/admin/product-images`.
pub async fn upload(
    State(state): State<AppState>,
    CurrentUser(current): CurrentUser,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, ApiProblem> {
    let mut multipart = multipart.map_err(|_| {
        problem(
            422,
            "invalid_upload",
            "Send the image as multipart form data.",
        )
    })?;

    let mut image: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(upload_too_large());
            }
            Err(_) => {
                return Err(problem(
                    422,
                    "invalid_upload",
                    "Send the image as multipart form data.",
                ));
            }
        };
        if field.name() != Some("image") || field.file_name().is_none() {
            continue;
        }
        let bytes = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(upload_too_large());
            }
            Err(_) => {
                return Err(problem(
                    400,
                    "invalid_upload",
                    "The image could not be read.",
                ));
            }
        };
        if bytes.len() > MAX_PRODUCT_IMAGE_BYTES {
            return Err(upload_too_large());
        }
        image = Some(bytes.to_vec());
        break;
    }
    let Some(body) = image else {
        return Err(problem(422, "invalid_upload", "Choose an image to upload."));
    };

    let Some(mime) = sniff_image_mime(&body) else {
        return Err(problem(
            422,
            "invalid_image",
            "Use a JPEG, PNG or WebP image.",
        ));
    };

    let now = Utc::now();
    let image_id = new_id("img");
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| problem(500, "internal_error", "The image could not be saved."))?;
    let saved = state
        .store
        .create_product_image(
            tx.conn(),
            &azahar_store::ProductImage {
                id: image_id.clone(),
                content_type: mime.to_string(),
                data: body.clone(),
                created_at: Some(now),
            },
            &current.id,
        )
        .await;
    let audited = match saved {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: "product_image.create".to_string(),
                        entity_type: "product_image".to_string(),
                        entity_id: image_id.clone(),
                        detail: format!("{{\"mime\":\"{mime}\"}}"),
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(problem(
                500,
                "internal_error",
                "The image could not be saved.",
            ));
        }
    }

    Ok(json_response(
        201,
        &json!({
            "image": {
                "id": image_id,
                "url": format!("/api/images/{image_id}"),
                "mime": mime,
                "bytes": body.len(),
            }
        }),
    ))
}

fn image_not_found() -> ApiProblem {
    problem(404, "not_found", "Image not found.")
}

/// `GET /api/images/{id}`.
pub async fn by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiProblem> {
    let id = id.trim().to_string();
    if !azahar_productrules::valid_image_id(&id) {
        return Err(image_not_found());
    }
    let image = match state.store.product_image(&id).await {
        Ok(image) => image,
        Err(StoreError::NotFound) => return Err(image_not_found()),
        Err(_) => return Err(internal_error()),
    };
    let length = match HeaderValue::from_str(&image.data.len().to_string()) {
        Ok(value) => value,
        Err(_) => return Err(internal_error()),
    };
    let content_type = HeaderValue::from_str(&image.content_type).map_err(|_| internal_error())?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_LENGTH, length),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("inline"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        Body::from(image.data),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing_matches_go_detect_content_type() {
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(
            sniff_image_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            Some("image/png")
        );
        let mut webp = b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec();
        assert_eq!(sniff_image_mime(&webp), Some("image/webp"));
        webp.truncate(12);
        assert_eq!(sniff_image_mime(&webp), None, "Go requires the VP bytes");
        assert_eq!(sniff_image_mime(b"not an image"), None);
        assert_eq!(sniff_image_mime(&[]), None);
    }
}
