use axum::{
    response::sse::{Event, KeepAlive, Sse},
};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

/// SSE event types
#[derive(Debug, Clone)]
pub enum SseEvent {
    Data(String),
    Done,
    Error(String),
}

/// Create an SSE response from a receiver channel
pub fn sse_response(
    rx: mpsc::Receiver<SseEvent>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = ReceiverStream::new(rx).filter_map(|event| match event {
        SseEvent::Data(data) => Some(Ok(Event::default().data(data))),
        SseEvent::Done => Some(Ok(Event::default().data("[DONE]"))),
        SseEvent::Error(msg) => Some(Ok(Event::default()
            .data(format!("[ERROR] {}", msg))
            .event("error"))),
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

/// Channel + SSE response pair
pub fn sse_channel() -> (
    mpsc::Sender<SseEvent>,
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
) {
    let (tx, rx) = mpsc::channel::<SseEvent>(64);
    (tx, sse_response(rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sse_channel() {
        let (tx, sse) = sse_channel();
        tx.send(SseEvent::Data("test".into())).await.unwrap();
        tx.send(SseEvent::Done).await.unwrap();
        // Sse implements IntoResponse, verified via type check
    }

    #[test]
    fn test_sse_event_debug() {
        let event = SseEvent::Data("hello".into());
        assert!(format!("{:?}", event).contains("hello"));
    }
}
