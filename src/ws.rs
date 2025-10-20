//! WebSocket based event gateway.

use axum::extract::ws::{CloseFrame, Message, WebSocket};

use duel_channel_model::{
    ApiError,
    battle::Battle,
    message::{Message as ApiMessage, server::NewBattle},
};

use sqlx::SqlitePool;

use tokio::{
    sync::broadcast::{self, Receiver, Sender, WeakSender, error::RecvError},
    task::JoinHandle,
};

use tracing::instrument;

use crate::{
    app::{AppError, error::AppErrorKind},
    auth::AuthenticatedUser,
};

/// The WebSocket state.
///
/// This serves as a master object that can lease handles to new websockets.
#[derive(Debug)]
pub struct Room {
    tx: Sender<RoomEvent>,
    _handle: JoinHandle<()>,
}

impl Room {
    /// Creates a new `Room`.
    ///
    /// This spins up a maintenance task for the room.
    pub fn new(db: SqlitePool) -> Room {
        let (tx, rx) = broadcast::channel(8);

        // create maintenance task
        let handle = tokio::spawn(run(db, tx.downgrade(), rx));

        Room {
            tx,
            _handle: handle,
        }
    }

    /// Sends an event to the room.
    pub fn send(&self, event: RoomEvent) {
        let _ = self.tx.send(event);
    }

    /// Serves a new client, with additional authentication information.
    ///
    /// **This commandeers the calling task!**
    pub fn serve(
        &self,
        ws: WebSocket,
        user: Option<AuthenticatedUser>,
    ) -> impl Future<Output = ()> + Send + 'static + use<> {
        serve(WebSocketState {
            ws,
            handle: self.get_handle(),
            user,
            closed: false,
        })
    }

    fn get_handle(&self) -> Handle {
        Handle {
            rx: self.tx.subscribe(),
        }
    }
}

/// An internal event given to a room.
#[derive(Clone, Debug)]
pub enum RoomEvent {
    /// Bets are open! Get those wagers in!
    NewBattle(Battle),
}

/// A handle to a room.
#[derive(Debug)]
pub struct Handle {
    /// Events produced by the room.
    pub rx: Receiver<RoomEvent>,
}

struct WebSocketState {
    ws: WebSocket,
    handle: Handle,
    user: Option<AuthenticatedUser>,
    closed: bool,
}

/// Serves a websocket.
async fn serve(mut state: WebSocketState) {
    while !state.closed {
        let WebSocketState { ws, handle, .. } = &mut state;

        tokio::select! {
            ev = ws.recv() => {
                match ev {
                    Some(Ok(msg)) => {
                        if let Err(err) = handle_message(&mut state, msg).await {
                            if err.is_internal() {
                                tracing::error!("ws error: {}", err);
                            }
                            let close_code = if err.is_internal() {
                                1011
                            } else {
                                1002
                            };
                            let err = serde_json::to_string(&err.to_api_error()).expect("valid json");
                            send_close_message(&mut state.ws, close_code, err).await;
                        }
                    }
                    // a fatal transfer error occured
                    Some(Err(err)) => {
                        tracing::info!("error receiving message: {}", err);
                        let err = serde_json::to_string(&ApiError {
                            message: "An internal server error occured.".into(),
                        }).expect("valid json");
                        send_close_message(ws, 1011, err).await;
                    }
                    // the websocket is closed!
                    None => break,
                }
            }
            ev = handle.rx.recv() => {
                match ev {
                    Ok(event) => {
                        if let Err(err) = handle_server_event(&mut state, event).await {
                            if err.is_internal() {
                                tracing::error!("ws error: {}", err);
                            }
                            let close_code = if err.is_internal() {
                                1011
                            } else {
                                1002
                            };
                            let err = serde_json::to_string(&err.to_api_error()).expect("valid json");
                            send_close_message(&mut state.ws, close_code, err).await;
                        }
                    }
                    // Lagged errors are fine
                    Err(RecvError::Lagged(err)) => {
                        tracing::warn!("ws lagged: {}", err);
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    // the websocket closes when it falls out of scope
}

/// Handles a message from the client.
#[instrument(skip(state))]
async fn handle_message(state: &mut WebSocketState, message: Message) -> Result<(), AppError> {
    let mut api_message = None::<ApiMessage>;

    match message {
        Message::Text(text) => api_message = Some(serde_json::from_str(&text)?),
        Message::Binary(bytes) => api_message = Some(serde_json::from_slice(&bytes)?),
        Message::Close(_close_frame) => {
            let err = serde_json::to_string(&ApiError {
                message: "Bye!".into(),
            })
            .expect("valid json");
            send_close_message(&mut state.ws, 1000, err).await;
            // Since the client sent the closed first, we don't
            // have to do anything else
            state.closed = true;
        }
        // Do not handle pings
        _ => (),
    }

    let _ = api_message;

    Ok(())
}

/// Handles an internal server event.
#[instrument(skip(state))]
async fn handle_server_event(state: &mut WebSocketState, ev: RoomEvent) -> Result<(), AppError> {
    match ev {
        RoomEvent::NewBattle(battle) => {
            // send battle information to user
            let new_battle = NewBattle(battle);
            let text =
                serde_json::to_string(&ApiMessage::NewBattle(new_battle)).expect("valid json");
            state
                .ws
                .send(Message::Text(text.into()))
                .await
                .map_err(AppErrorKind::WebSocket)?;
        }
    }

    Ok(())
}

async fn send_close_message(ws: &mut WebSocket, code: u16, reason: impl Into<String>) {
    let _ = ws
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into().into(),
        })))
        .await;
}

/// An administrative task for [`Room`].
#[instrument(skip(db, tx, rx))]
async fn run(db: SqlitePool, tx: WeakSender<RoomEvent>, mut rx: Receiver<RoomEvent>) {
    loop {
        let ev = rx.recv().await;

        match ev {
            Ok(RoomEvent::NewBattle(battle)) => {
                // New battle started! Wait for
            }
            // Lagged errors are fine
            Err(RecvError::Lagged(err)) => {
                tracing::warn!("ws room lagged: {}", err);
            }
            Err(RecvError::Closed) => break,
        }
    }

    tracing::info!("room closed")
}
