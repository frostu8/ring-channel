//! Application error that may occur during the processing of a request.
//!
//! See [`AppError`].

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use axum::{
    extract::rejection::JsonRejection,
    response::{IntoResponse, Response},
};

use derive_more::{Display, From};

use http::StatusCode;

use crate::{app::AppJson, model::ApiError};

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
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut internal_error = None;

        let (status, mut error) = match self.kind {
            AppErrorKind::Json(error) => (
                StatusCode::BAD_REQUEST,
                ApiError {
                    message: error.to_string(),
                },
            ),
            // fallthrough for internal server errors not turned into user
            // errors here
            error => {
                internal_error = Some(error);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiError {
                        message: "An internal server error occured.".into(),
                    },
                )
            }
        };

        // replace error message
        if let Some(message) = self.message {
            error.message = message;
        }

        let mut response = (status, AppJson(error)).into_response();
        if let Some(error) = internal_error {
            response.extensions_mut().insert(Arc::new(error));
        }
        response
    }
}
