//! Thin protocol wrapper for [`WebSocket`].

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::ws::{self, CloseFrame};

use derive_more::{Display, Error, From};

use futures_core::ready;
use futures_util::{Sink, SinkExt, Stream, StreamExt};

use ring_channel_model::ApiError;
use ring_channel_model::message::Message;

use pin_project::pin_project;

use ring_channel_model::message::client::Heartbeat;
use ring_channel_model::message::server::HeartbeatAck;

use tokio::time::{Sleep, sleep};

/// Gives clients some time to send heartbeats over unstable network
/// conditions.
pub const HEARTBEAT_GRACE_DURATION: Duration = Duration::from_secs(5);

/// A connection to a client.
#[derive(Debug)]
#[pin_project]
pub struct WebSocket {
    #[pin]
    inner: ws::WebSocket,
    close_timeout: Duration,

    // Heartbeats
    heartbeater: Heartbeater,
    heartbeat_stage: HeartbeatStage,

    // A close frame was recieved from the client
    closed_client: bool,
    // A close frame was sent to the client
    closed_server: bool,
    close_stage: CloseStage,
}

#[derive(Debug)]
enum CloseStage {
    Running,
    // waiting for a close frame from the client
    Wait(Pin<Box<Sleep>>),
    Flushing,
    Closing,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
enum HeartbeatStage {
    None,
    Flushing,
}

impl WebSocket {
    /// Checks if the websocket is closed.
    pub fn is_closed(&self) -> bool {
        matches!(self.close_stage, CloseStage::Closed)
    }

    /// Sends a message over the websocket.
    pub async fn send(&mut self, message: &Message) -> Result<(), Error> {
        <WebSocket as SinkExt<&Message>>::send(self, message).await
    }

    /// Receives a mess  --> src/room/mod.rs:20:21age over the websocket.
    pub async fn recv(&mut self) -> Option<Result<Message, Error>> {
        <WebSocket as StreamExt>::next(self).await
    }

    /// Sends a close message over the websocket.
    ///
    /// This starts the closing process.
    pub async fn send_close(&mut self, code: u16, error: &ApiError) -> Result<(), Error> {
        let msg = serde_json::to_string(error)?;
        self.inner
            .send(ws::Message::Close(Some(CloseFrame {
                code,
                reason: msg.into(),
            })))
            .await?;
        // TODO: magic number?
        self.close_stage = CloseStage::Wait(Box::pin(tokio::time::sleep(self.close_timeout)));
        self.closed_server = true;
        Ok(())
    }

    fn preprocess_message(self: Pin<&mut Self>, msg: &Message) -> Result<(), Error> {
        let this = self.project();

        match msg {
            Message::Heartbeat(heartbeat) => {
                if let Some(resp) = this.heartbeater.ack(heartbeat) {
                    let message: Message = resp.into();
                    let text = serde_json::to_string(&message)?;
                    this.inner.start_send(ws::Message::Text(text.into()))?;
                    *this.heartbeat_stage = HeartbeatStage::Flushing;
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn poll_close_inner(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        loop {
            let mut this = self.as_mut().project();

            match this.close_stage {
                CloseStage::Running => return Poll::Pending,
                CloseStage::Wait(timeout) => {
                    if timeout.as_mut().poll(cx).is_ready() {
                        // client is taking too long 💢 they need correction.
                        *this.close_stage = CloseStage::Closing;
                        continue;
                    }

                    let ev = ready!(this.inner.as_mut().poll_next(cx));

                    // wait for a close frame
                    match ev {
                        Some(Ok(ws::Message::Close(_close_frame))) => {
                            *this.close_stage = CloseStage::Closing;
                            *this.closed_client = true;
                        }
                        // ignore any events
                        Some(Ok(_)) => (),
                        Some(Err(err)) => return Poll::Ready(Err(err.into())),
                        None => return Poll::Ready(Ok(())),
                    }
                }
                CloseStage::Flushing => {
                    ready!(this.inner.poll_flush(cx))?;
                    *this.closed_server = true;

                    // Check if the client sent their close frame
                    if *this.closed_client {
                        *this.close_stage = CloseStage::Closing;
                    } else {
                        *this.close_stage = CloseStage::Wait(Box::pin(sleep(*this.close_timeout)));
                    }
                }
                CloseStage::Closing => {
                    ready!(this.inner.poll_close(cx))?;
                    *this.close_stage = CloseStage::Closed;
                }
                CloseStage::Closed => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // fuse
            match self.as_mut().poll_close_inner(cx) {
                Poll::Ready(Ok(())) => return Poll::Ready(None),
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err.into()))),
                Poll::Pending => (),
            }

            let mut this = self.as_mut().project();

            if *this.heartbeat_stage == HeartbeatStage::Flushing {
                // finish flush of heartbeat before receiving
                ready!(this.inner.as_mut().poll_flush(cx))?;
                *this.heartbeat_stage = HeartbeatStage::None;
            }

            match this.heartbeater.timeout.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    // uh oh! client didn't send their government-mandated
                    // pings.
                    let reason = serde_json::to_string(&ApiError {
                        message: "Failed to heartbeat; disconnecting".into(),
                    })?;
                    let frame = CloseFrame {
                        code: 1002,
                        reason: reason.into(),
                    };
                    this.inner
                        .as_mut()
                        .start_send(ws::Message::Close(Some(frame)))?;
                    *this.close_stage = CloseStage::Flushing;
                }
                Poll::Pending => (),
            }

            let ev = ready!(this.inner.as_mut().poll_next(cx));

            match ev {
                Some(Ok(ws::Message::Text(text))) => {
                    let message = serde_json::from_str::<Message>(&text)?;
                    self.preprocess_message(&message)?;
                    return Poll::Ready(Some(Ok(message)));
                }
                Some(Ok(ws::Message::Binary(bytes))) => {
                    let message = serde_json::from_slice::<Message>(&bytes)?;
                    self.preprocess_message(&message)?;
                    return Poll::Ready(Some(Ok(message)));
                }
                Some(Ok(ws::Message::Close(_close_frame))) => {
                    let reason = serde_json::to_string(&ApiError {
                        message: "Bye!".into(),
                    })?;
                    let frame = CloseFrame {
                        code: 1001,
                        reason: reason.into(),
                    };
                    *this.closed_client = true;
                    if let Err(_err) = this.inner.start_send(ws::Message::Close(Some(frame))) {
                        // ignore any send after closing errors
                        *this.close_stage = CloseStage::Closed;
                        return Poll::Ready(None);
                    } else {
                        *this.close_stage = CloseStage::Flushing;
                    }
                }
                // ping and pong unused
                Some(Ok(_)) => (),
                Some(Err(err)) => return Poll::Ready(Some(Err(err.into()))),
                None => return Poll::Ready(None),
            }
        }
    }
}

impl Sink<&Message> for WebSocket {
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        ready!(this.inner.poll_ready(cx))?;
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: &Message) -> Result<(), Self::Error> {
        let msg = serde_json::to_string(item)?;

        let this = self.project();
        this.inner
            .start_send(ws::Message::Text(msg.into()))
            .map_err(Error::from)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        ready!(this.inner.poll_flush(cx))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        ready!(this.inner.poll_close(cx))?;
        Poll::Ready(Ok(()))
    }
}

impl From<ws::WebSocket> for WebSocket {
    fn from(inner: ws::WebSocket) -> Self {
        WebSocket {
            inner,
            heartbeater: Heartbeater::default(),
            heartbeat_stage: HeartbeatStage::None,
            close_timeout: Duration::from_secs(5),
            close_stage: CloseStage::Running,
            closed_client: false,
            closed_server: false,
        }
    }
}

/// Socket heartbeater.
#[derive(Debug)]
pub struct Heartbeater {
    interval: Duration,
    timeout: Pin<Box<Sleep>>,
    seq: i32,
}

impl Heartbeater {
    /// Creates a new `Heartbeater`.
    pub fn new(interval: Duration) -> Heartbeater {
        Heartbeater {
            interval,
            timeout: Box::pin(sleep(interval + HEARTBEAT_GRACE_DURATION)),
            seq: 0,
        }
    }

    /// Acknowledges a heartbeat.
    pub fn ack(&mut self, heartbeat: &Heartbeat) -> Option<HeartbeatAck> {
        // ignore invalid sequence heartbeats
        if heartbeat.seq > self.seq {
            // reset heartbeat timers
            self.timeout = Box::pin(sleep(self.interval + HEARTBEAT_GRACE_DURATION));
            self.seq = heartbeat.seq;

            // send acknowledgement
            Some(HeartbeatAck { seq: heartbeat.seq })
        } else {
            None
        }
    }

    /// Waits for a timeout.
    pub async fn timeout(&mut self) {
        (&mut self.timeout).await
    }
}

impl Default for Heartbeater {
    fn default() -> Self {
        Heartbeater::new(Duration::from_secs(30))
    }
}

/// A [`WebSocket`] error.
#[derive(Debug, Display, Error, From)]
pub enum Error {
    /// A websocket error occured.
    #[display("{_0}")]
    Ws(axum::Error),
    /// A serialization error occured.
    #[display("{_0}")]
    Serde(serde_json::Error),
}
