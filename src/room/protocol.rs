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

use tokio::time::Sleep;

/// A connection to a client.
#[derive(Debug)]
#[pin_project]
pub struct WebSocket {
    #[pin]
    inner: ws::WebSocket,
    close_timeout: Duration,
    // A close frame was sent to the client
    closed: bool,
    close_stage: CloseStage,
}

#[derive(Debug)]
enum CloseStage {
    // waiting for a close frame from the client
    Wait(Pin<Box<Sleep>>),
    Flushing,
    Closing,
    Closed,
}

impl WebSocket {
    /// Checks if the websocket is closed.
    pub fn is_closed(&self) -> bool {
        self.closed && matches!(self.close_stage, CloseStage::Closed)
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
        self.closed = true;
        Ok(())
    }

    fn poll_close_inner(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        loop {
            let mut this = self.as_mut().project();

            match this.close_stage {
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
                        }
                        // ignore any events
                        Some(Ok(_)) => (),
                        Some(Err(err)) => return Poll::Ready(Err(err.into())),
                        None => return Poll::Ready(Ok(())),
                    }
                }
                CloseStage::Flushing => {
                    ready!(this.inner.poll_flush(cx))?;
                    *this.close_stage = CloseStage::Closing;
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
            let mut this = self.as_mut().project();

            // fuse
            if *this.closed {
                ready!(self.as_mut().poll_close_inner(cx))?;
                return Poll::Ready(None);
            }

            let ev = ready!(this.inner.as_mut().poll_next(cx));

            match ev {
                Some(Ok(ws::Message::Text(text))) => {
                    let message = serde_json::from_str::<Message>(&text).map_err(Error::from);
                    return Poll::Ready(Some(message));
                }
                Some(Ok(ws::Message::Binary(bytes))) => {
                    let message = serde_json::from_slice::<Message>(&bytes).map_err(Error::from);
                    return Poll::Ready(Some(message));
                }
                Some(Ok(ws::Message::Close(_close_frame))) => {
                    let reason = serde_json::to_string(&ApiError {
                        message: "Bye!".into(),
                    })?;
                    let frame = CloseFrame {
                        code: 1001,
                        reason: reason.into(),
                    };
                    this.inner.start_send(ws::Message::Close(Some(frame)))?;
                    self.close_stage = CloseStage::Flushing;
                    self.closed = true;
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
            close_timeout: Duration::from_secs(5),
            close_stage: CloseStage::Wait(Box::pin(tokio::time::sleep(Duration::new(0, 0)))),
            closed: false,
        }
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
