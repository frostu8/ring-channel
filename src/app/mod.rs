//! Application interface and state.

pub mod error;

pub use error::AppError;

use axum::{
    Json,
    extract::FromRequest,
    response::{IntoResponse, Response},
};

use derive_more::Deref;

use sqlx::SqlitePool;

/// Shared app state.
///
/// Cheaply cloneable.
#[derive(Clone, Debug)]
pub struct AppState {
    /// The database connection pool.
    pub db: SqlitePool,
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
