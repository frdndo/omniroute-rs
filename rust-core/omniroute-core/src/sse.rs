use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use futures::stream::Stream;
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::mpsc;

/// SSE event types
#[derive(Debug, Clone)]
pub enum SseEvent {
    Data(String),
    Done,
    Error(String),
}

/// A stream of SSE events that can serve as an Axum response
pub struct SseStream {
    rx: mpsc::Receiver<SseEvent>,
}

impl SseStream {
    pub fn new(rx: mpsc::Receiver<SseEvent>) -> Self {
        Self { rx }
    }
}

impl Stream for SseStream {
    type Item = Result<Body, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(SseEvent::Data(data))) => {
                let body = Body::from(format!("data: {}\n\n", data));
                Poll::Ready(Some(Ok(body)))
            }
            Poll::Ready(Some(SseEvent::Done)) => {
                let body = Body::from("data: [DONE]\n\n");
                Poll::Ready(Some(Ok(body)))
            }
            Poll::Ready(Some(SseEvent::Error(msg))) => {
                let body = Body::from(format!(
                    "data: [ERROR] {}\n\nevent: error\ndata: {}\n\n",
                    msg, msg
                ));
                Poll::Ready(Some(Ok(body)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl IntoResponse for SseStream {
    fn into_response(self) -> Response {
        Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("X-Accel-Buffering", "no")
            .body(Body::from_stream(self))
            .unwrap()
    }
}

/// Create a channel pair for SSE streaming
pub fn sse_channel() -> (mpsc::Sender<SseEvent>, SseStream) {
    let (tx, rx) = mpsc::channel::<SseEvent>(64);
    (tx, SseStream::new(rx))
}

/// SSE keep-alive ping
pub fn sse_heartbeat(tx: mpsc::Sender<SseEvent>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            if tx.send(SseEvent::Data(": keepalive".into())).await.is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sse_channel_send_receive() {
        let (tx, mut stream) = sse_channel();
        tx.send(SseEvent::Data("test message".into()))
            .await
            .unwrap();
        tx.send(SseEvent::Done).await.unwrap();
        drop(tx);

        use futures::StreamExt;
        let items: Vec<_> = stream.collect().await;
        assert!(!items.is_empty());
    }

    #[tokio::test]
    async fn test_sse_headers() {
        let (_, stream) = sse_channel();
        let resp = stream.into_response();
        assert_eq!(
            resp.headers().get("Content-Type").unwrap(),
            "text/event-stream"
        );
        assert_eq!(resp.headers().get("Cache-Control").unwrap(), "no-cache");
    }

    #[test]
    fn test_sse_event_debug() {
        let event = SseEvent::Data("hello".into());
        assert!(format!("{:?}", event).contains("hello"));
    }
}
