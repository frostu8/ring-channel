//! Application error that may occur during the processing of a request.
//!
//! See [`AppError`].

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use axum::{
    extract::rejection::{FormRejection, JsonRejection},
    response::{IntoResponse, Response},
};

use derive_more::{Display, From};

use http::StatusCode;

use ring_channel_model::ApiError;
use uuid::Uuid;

use crate::app::AppJson;

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Application error that may occur during the processing of a request.
///
/// This includes both internal errors and user errors.
#[derive(Debug)]
pub struct AppError {
    kind: AppErrorKind,
    message: Option<String>,
}

impl AppError {
    /// Constucts an internal error.
    pub fn new<E: Error + Send + Sync + 'static>(e: E) -> AppError {
        AppError {
            kind: AppErrorKind::Other(Box::new(e)),
            message: None,
        }
    }

    /// Constructs a new not found error.
    pub fn not_found(message: impl Into<String>) -> AppError {
        AppError {
            kind: AppErrorKind::NotFound,
            message: Some(message.into()),
        }
    }

    /// Adds a custom message to the error.
    pub fn with_message(self, message: impl Into<String>) -> AppError {
        AppError {
            message: Some(message.into()),
            ..self
        }
    }

    /// The inner [`AppErrorKind`] of the error.
    pub fn kind(&self) -> &AppErrorKind {
        &self.kind
    }

    /// Discards the error message, unwrapping the inner error.
    pub fn into_kind(self) -> AppErrorKind {
        self.kind
    }

    /// Checks if the error is an internal error.
    pub fn is_internal(&self) -> bool {
        matches!(
            self.kind,
            AppErrorKind::Database(_)
                | AppErrorKind::WebSocket(_)
                | AppErrorKind::Session(_)
                | AppErrorKind::HttpClient(_)
                | AppErrorKind::Discord(_)
                | AppErrorKind::OutOfIds
                | AppErrorKind::Other(_)
        )
    }

    /// Converts the app error to an API error.
    pub fn to_api_error(self) -> ApiError {
        let (_, error) = self.to_status_and_api_error();
        error
    }

    fn to_status_and_api_error(self) -> (StatusCode, ApiError) {
        let (status, mut error) = match self.kind {
            AppErrorKind::NotFound => (
                StatusCode::NOT_FOUND,
                ApiError {
                    message: "Resource not found".into(),
                },
            ),
            error_kind @ AppErrorKind::AlreadyConcluded(_) => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: error_kind.to_string(),
                },
            ),
            AppErrorKind::Json(error) => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: error.to_string(),
                },
            ),
            AppErrorKind::SerdeJson(error) => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: error.to_string(),
                },
            ),
            AppErrorKind::Form(error) => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: error.to_string(),
                },
            ),
            AppErrorKind::UnsupportedContentType(mime) => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: format!("Unrecognized MIME type: {}", mime),
                },
            ),
            AppErrorKind::MissingContentType => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: "Missing request content type".into(),
                },
            ),
            AppErrorKind::InvalidState { .. } => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: "Invalid state sent".into(),
                },
            ),
            AppErrorKind::SessionFetch((code, message)) => (
                code,
                ApiError {
                    message: message.into(),
                },
            ),
            // fallthrough for internal server errors not turned into user
            // errors here
            _error_kind => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError {
                    message: "An internal server error occured".into(),
                },
            ),
        };

        // replace error message
        if let Some(message) = self.message {
            error.message = message;
        }

        (status, error)
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.message.as_ref() {
            Some(msg) => f.write_str(msg),
            None => Display::fmt(&self.kind, f),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            AppErrorKind::Json(err) => Some(err),
            AppErrorKind::Form(err) => Some(err),
            AppErrorKind::Database(err) => Some(err),
            AppErrorKind::Session(err) => Some(err),
            AppErrorKind::WebSocket(err) => Some(err),
            AppErrorKind::HttpClient(err) => Some(err),
            AppErrorKind::Discord(err) => Some(err),
            AppErrorKind::Other(err) => err.source(),
            _ => None,
        }
    }
}

impl<T> From<T> for AppError
where
    T: Into<AppErrorKind>,
{
    fn from(value: T) -> Self {
        AppError {
            kind: value.into(),
            message: None,
        }
    }
}

/// The specific kind of error that happened.
#[derive(Debug, Display, From)]
#[non_exhaustive]
pub enum AppErrorKind {
    /// The request's JSON payload was malformed or invalid.
    #[display("{_0}")]
    Json(JsonRejection),
    /// A JSON-related error occured.
    SerdeJson(serde_json::Error),
    /// The request's urlencoded payload was malformed or invalid.
    #[display("{_0}")]
    Form(FormRejection),
    /// A resource was not found.
    #[display("Resource not found")]
    NotFound,
    /// A battle with the given UUID already concluded.
    #[display("Battle {_0} concluded")]
    #[from(ignore)]
    AlreadyConcluded(Uuid),
    /// A content type was not provided.
    MissingContentType,
    /// The server cannot serve this content type.
    #[from(ignore)]
    UnsupportedContentType(String),
    /// An error with the session occured.
    #[display("{} {}: {}", _0.0, _0.0.canonical_reason().unwrap_or("Error"), _0.1)]
    #[from(ignore)]
    SessionFetch((StatusCode, &'static str)),
    /// An error getting session data occured.
    Session(tower_sessions::session::Error),
    /// A request to the Discord API failed.
    Discord(twilight_http::Error),
    /// An HTTP request failed.
    HttpClient(reqwest::Error),
    /// Invalid state in OAuth2 grant flow detected.
    #[from(ignore)]
    InvalidState { state: String },
    /// A websocket error occured.
    #[from(ignore)]
    WebSocket(axum::Error),
    /// An unhandled database error occured.
    Database(sqlx::Error),
    /// The application failed to generate a unique id.
    #[display("Ran out of ids")]
    OutOfIds,
    /// An error happened.
    ///
    /// Only the message is preserved! All errors of this kind are internal.
    /// Use as a last resort.
    #[from(ignore)]
    Other(BoxError),
}

impl IntoResponse for AppError {
    fn into_response(mut self) -> Response {
        let mut internal_error = None;

        let (status, error) = if self.is_internal() {
            internal_error = Some(AppError {
                kind: self.kind,
                message: self.message.take(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError {
                    message: "An internal server error occured.".into(),
                },
            )
        } else {
            self.to_status_and_api_error()
        };

        let mut response = (status, AppJson(error)).into_response();
        if let Some(error) = internal_error {
            response.extensions_mut().insert(Arc::new(error));
        }
        response
    }
}
