//! Application interface and state.

pub mod error;

use std::sync::Arc;

pub use error::AppError;

use axum::{
    Form, Json, RequestExt as _,
    extract::{FromRequest, Request},
    response::{IntoResponse, Response},
};

use derive_more::Deref;

use http::header;

use serde::de::DeserializeOwned;

use sqlx::SqlitePool;

use crate::{app::error::AppErrorKind, ws};

/// Shared app state.
///
/// Cheaply cloneable.
#[derive(Clone, Debug)]
pub struct AppState {
    /// The database connection pool.
    pub db: SqlitePool,
    /// The WebSocket room.
    pub room: Arc<ws::Room>,
}

/// Selective body extractor.
///
/// The duel-channel API can accept both JSON and urlencoded bodies.
#[derive(Deref)]
pub struct Payload<T>(pub T);

impl<S, T> FromRequest<S> for Payload<T>
where
    T: DeserializeOwned + 'static,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // switch on content type
        let content_type = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppErrorKind::MissingContentType)?;

        match content_type {
            "application/x-www-form-urlencoded" => {
                let AppForm(form) = req.extract_with_state::<AppForm<T>, _, _>(state).await?;
                Ok(Payload(form))
            }
            "application/json" => {
                let AppJson(json) = req.extract_with_state::<AppJson<T>, _, _>(state).await?;
                Ok(Payload(json))
            }
            mime => Err(AppErrorKind::UnsupportedContentType(mime.to_owned()).into()),
        }
    }
}

/// App Form extractor and responder.
#[derive(Deref, FromRequest)]
#[from_request(via(Form), rejection(AppError))]
pub struct AppForm<T>(pub T);

impl<T> IntoResponse for AppForm<T>
where
    Form<T>: IntoResponse,
{
    fn into_response(self) -> Response {
        Form(self.0).into_response()
    }
}

/// App JSON extractor and responder.
#[derive(Deref, FromRequest)]
#[from_request(via(Json), rejection(AppError))]
pub struct AppJson<T>(pub T);

impl<T> IntoResponse for AppJson<T>
where
    Json<T>: IntoResponse,
{
    fn into_response(self) -> Response {
        Json(self.0).into_response()
    }
}
