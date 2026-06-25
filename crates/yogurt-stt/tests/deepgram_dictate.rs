use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use yogurt_stt::deepgram::DeepgramStt;

fn results(text: &str) -> Message {
    Message::Text(
        serde_json::json!({
            "type": "Results",
            "is_final": true,
            "channel": { "alternatives": [{ "transcript": text }] },
        })
        .to_string(),
    )
}

async fn mock() -> (TcpListener, DeepgramStt) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut dg = DeepgramStt::new("fake-key");
    dg.base_url = format!("ws://{}", listener.local_addr().unwrap());
    (listener, dg)
}

#[tokio::test]
async fn it_streams_the_clip_and_joins_the_finals_after_close_stream() {
    let (listener, dg) = mock().await;
    let (bytes_tx, bytes_rx) = tokio::sync::oneshot::channel();
    let (uri_tx, uri_rx) = tokio::sync::oneshot::channel::<String>();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // tungstenite's handshake callback fixes the large error type.
        #[allow(clippy::result_large_err)]
        let accept = |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
            uri_tx.send(req.uri().to_string()).unwrap();
            Ok(resp)
        };
        let (mut write, mut read) = tokio_tungstenite::accept_hdr_async(stream, accept)
            .await
            .unwrap()
            .split();
        let mut bytes = 0;
        while let Some(Ok(msg)) = read.next().await {
            match msg {
                Message::Binary(b) => bytes += b.len(),
                Message::Text(t) if t.contains("CloseStream") => break,
                _ => {}
            }
        }
        bytes_tx.send(bytes).unwrap();
        write.send(results("hello there,")).await.unwrap();
        write.send(results("")).await.unwrap();
        write.send(results("general Kenobi.")).await.unwrap();
        write
            .send(Message::Text(r#"{"type":"Metadata"}"#.into()))
            .await
            .unwrap();
        // Held open, so only the Metadata frame can end the dictation.
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(vec![1i16; 160]).unwrap();
    tx.send(vec![2i16; 160]).unwrap();
    drop(tx);
    let text = tokio::time::timeout(Duration::from_secs(2), dg.dictate(rx, &["Marivex".into()]))
        .await
        .expect("Metadata ends the dictation")
        .unwrap();
    assert_eq!(text, "hello there, general Kenobi.");
    assert!(uri_rx.await.unwrap().ends_with("&keyterm=Marivex"));
    assert_eq!(
        bytes_rx.await.unwrap(),
        640,
        "both frames went out as linear16"
    );
    server.abort();
}

#[tokio::test]
async fn a_socket_closed_during_the_clip_is_an_error() {
    let (listener, dg) = mock().await;
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.close(None).await.unwrap();
    });
    let (_tx, rx) = mpsc::unbounded_channel();
    let err = dg.dictate(rx, &[]).await.unwrap_err();
    assert!(err.to_string().contains("during the clip"), "{err}");
}

#[tokio::test]
async fn a_rejected_key_is_an_error() {
    let (listener, dg) = mock().await;
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await;
        let _ = stream
            .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n")
            .await;
        let _ = stream.shutdown().await;
    });
    let (_tx, rx) = mpsc::unbounded_channel();
    let err = dg.dictate(rx, &[]).await.unwrap_err();
    assert!(err.to_string().contains("rejected the key"), "{err}");
}

#[tokio::test]
async fn dropping_the_dictation_closes_the_socket() {
    let (listener, dg) = mock().await;
    let (closed_tx, closed) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) if t.contains("CloseStream") => {
                    panic!("a cancel must not ask for the result")
                }
                Some(Ok(_)) => {}
                _ => break,
            }
        }
        let _ = closed_tx.send(());
    });
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(vec![0i16; 160]).unwrap();
    let task = tokio::spawn(async move { dg.dictate(rx, &[]).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    task.abort();
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .expect("the server sees the socket close")
        .unwrap();
    drop(tx);
}

#[tokio::test]
async fn a_socket_closed_after_close_stream_without_metadata_is_an_error() {
    let (listener, dg) = mock().await;
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(msg)) = ws.next().await {
            if matches!(msg, Message::Text(ref t) if t.contains("CloseStream")) {
                break;
            }
        }
        ws.close(None).await.unwrap();
    });
    let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();
    drop(tx);
    let err = dg.dictate(rx, &[]).await.unwrap_err();
    assert!(err.to_string().contains("before its final result"), "{err}");
}
