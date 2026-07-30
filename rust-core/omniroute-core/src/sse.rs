use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::convert::Infallible;
use tokio::sync::mpsc;

/// SSE event types
#[derive(Debug, Clone)]
pub enum SseEvent {
    Data(String),
    Done,
    Error(String),
}

/// A stream of SSE events
pub struct SseStream {
    rx: mpsc::Receiver<SseEvent>,
}

impl SseStream {
    pub fn new(rx: mpsc::Receiver<SseEvent>) -> Self {
        Self { rx }
    }
}

impl Stream for SseStream {
    type Item = Result<String, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(SseEvent::Data(data))) => {
                Poll::Ready(Some(Ok(format!("data: {}\n\n", data))))
            }
            Poll::Ready(Some(SseEvent::Done)) => Poll::Ready(Some(Ok("data: [DONE]\n\n".into()))),
            Poll::Ready(Some(SseEvent::Error(msg))) => Poll::Ready(Some(Ok(format!(
                "data: [ERROR] {}\n\nevent: error\ndata: {}\n\n",
                msg, msg
            )))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl IntoResponse for SseStream {
    fn into_response(self) -> Response {
        let stream = futures::stream::unfold(self, |mut s| async {
            match s.rx.recv().await {
                Some(SseEvent::Data(data)) => Some((Ok(format!("data: {}\n\n", data)), s)),
                Some(SseEvent::Done) => Some((Ok("data: [DONE]\n\n".into()), s)),
                Some(SseEvent::Error(msg)) => Some((
                    Ok(format!(
                        "data: [ERROR] {}\n\nevent: error\ndata: {}\n\n",
                        msg, msg
                    )),
                    s,
                )),
                None => None,
            }
        });

        Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("X-Accel-Buffering", "no")
            .status(StatusCode::OK)
            .body(axum::body::Body::from_stream(stream))
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
    async fn test_sse_channel() {
        let (tx, mut stream) = sse_channel();
        tx.send(SseEvent::Data("test".into())).await.unwrap();
        tx.send(SseEvent::Done).await.unwrap();
        drop(tx);

        use futures::StreamExt;
        let items: Vec<_> = stream.collect().await;
        assert!(!items.is_empty());
        assert_eq!(items[0].as_ref().unwrap(), "data: test\n\n");
    }

    #[test]
    fn test_sse_heartbeat_handle() {
        let (tx, _) = sse_channel();
        let handle = sse_heartbeat(tx);
        assert!(!handle.is_finished());
    }

    #[test]
    fn test_sse_event_debug() {
        let event = SseEvent::Data("hello".into());
        assert!(format!("{:?}", event).contains("hello"));
    }
}
