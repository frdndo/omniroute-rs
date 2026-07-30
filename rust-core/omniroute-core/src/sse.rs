use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tokio::sync::mpsc;
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
        let (tx, _stream) = sse_channel();
        tx.send(SseEvent::Data("test".into())).await.unwrap();
        tx.send(SseEvent::Done).await.unwrap();
        // Stream consumed via IntoResponse, not collect
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
