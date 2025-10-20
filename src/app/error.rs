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

use crate::app::AppJson;

/// Application error that may occur during the processing of a request.
///
/// This includes both internal errors and user errors.
#[derive(Debug)]
pub struct AppError {
    kind: AppErrorKind,
    message: Option<String>,
}

impl AppError {
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
            AppErrorKind::Database(_) | AppErrorKind::WebSocket(_)
        )
    }

    /// Converts the app error to an API error.
    pub fn to_api_error(self) -> ApiError {
        let (_, error) = self.to_status_and_api_error();
        error
    }

    fn to_status_and_api_error(self) -> (StatusCode, ApiError) {
        let (status, mut error) = match self.kind {
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
                    message: format!("Unrecognized MIME type: {}.", mime),
                },
            ),
            AppErrorKind::MissingContentType => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: "Missing request content type.".into(),
                },
            ),
            // fallthrough for internal server errors not turned into user
            // errors here
            _error_kind => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError {
                    message: "An internal server error occured.".into(),
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
            AppErrorKind::WebSocket(err) => Some(err),
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
    /// A content type was not provided.
    MissingContentType,
    /// The server cannot serve this content type.
    UnsupportedContentType(String),
    /// A websocket error occured.
    #[from(ignore)]
    WebSocket(axum::Error),
    /// An unhandled database error occured.
    Database(sqlx::Error),
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
