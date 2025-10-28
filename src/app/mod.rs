//! Application interface and state.

pub mod error;

use std::sync::Arc;

pub use error::AppError;

use axum_valid::{Garde, GardeRejection, HasValidate};

use axum::{
    Form, Json, RequestExt as _, RequestPartsExt as _,
    extract::{FromRef, FromRequest, FromRequestParts, Request},
    response::{IntoResponse, Response},
};

use derive_more::Deref;

use garde::Validate;
use http::{header, request::Parts};

use serde::de::DeserializeOwned;

use sqlx::SqlitePool;

use crate::{app::error::AppErrorKind, config::Config, room};

/// Shared app state.
///
/// Cheaply cloneable.
#[derive(Clone, Debug)]
pub struct AppState {
    /// The database connection pool.
    pub db: SqlitePool,
    /// The WebSocket room.
    pub room: room::Room,
    /// Server config.
    ///
    /// May be missing secrets as they are taken at initialization.
    pub config: Arc<Config>,
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

/// App Garde extrarctor.
#[derive(Deref)]
pub struct AppGarde<T>(pub T);

impl<S, T> FromRequestParts<S> for AppGarde<T>
where
    S: Send + Sync,
    T: FromRequestParts<S> + HasValidate + 'static,
    AppError: From<<T as FromRequestParts<S>>::Rejection>,
    <T as HasValidate>::Validate: Validate,
    <<T as HasValidate>::Validate as Validate>::Context: Send + Sync + FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let valid = parts.extract_with_state::<Garde<T>, S>(state).await;

        match valid {
            Ok(Garde(valid)) => Ok(AppGarde(valid)),
            Err(GardeRejection::Valid(garde)) => Err(AppErrorKind::Garde(garde).into()),
            Err(GardeRejection::Inner(err)) => Err(err.into()),
        }
    }
}

impl<S, T> FromRequest<S> for AppGarde<T>
where
    S: Send + Sync,
    T: FromRequest<S> + HasValidate + 'static,
    AppError: From<<T as FromRequest<S>>::Rejection>,
    <T as HasValidate>::Validate: Validate,
    <<T as HasValidate>::Validate as Validate>::Context: Send + Sync + FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let valid = request.extract_with_state::<Garde<T>, S, _>(state).await;

        match valid {
            Ok(Garde(valid)) => Ok(AppGarde(valid)),
            Err(GardeRejection::Valid(garde)) => Err(AppErrorKind::Garde(garde).into()),
            Err(GardeRejection::Inner(err)) => Err(err.into()),
        }
    }
}

impl<T> IntoResponse for AppGarde<T>
where
    T: IntoResponse,
{
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

/// App Form extractor and responder.
#[derive(Deref, FromRequest)]
#[from_request(via(Form), rejection(AppError))]
pub struct AppForm<T>(pub T);

impl<T> HasValidate for AppForm<T> {
    type Validate = T;

    fn get_validate(&self) -> &Self::Validate {
        &self.0
    }
}

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

impl<T> HasValidate for AppJson<T> {
    type Validate = T;

    fn get_validate(&self) -> &Self::Validate {
        &self.0
    }
}

impl<T> IntoResponse for AppJson<T>
where
    Json<T>: IntoResponse,
{
    fn into_response(self) -> Response {
        Json(self.0).into_response()
    }
}
