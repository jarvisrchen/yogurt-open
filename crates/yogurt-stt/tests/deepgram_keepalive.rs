//! A Deepgram socket whose audio stops sends Finalize, then keeps itself open with KeepAlive, after one silent frame if it never sent audio.

use futures_util::StreamExt;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use yogurt_stt::{deepgram::DeepgramStt, AudioChunk, Channel, Stt, TranscriptEvent};

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_socket_sends_keepalive_and_a_silent_frame_first_if_it_never_sent_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // `DeepgramStt::start` opens mic first, then system.
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel::<(usize, Vec<Message>)>();
    let server = tokio::spawn(async move {
        for n in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let seen_tx = seen_tx.clone();
            tokio::spawn(async move {
                let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                let (_write, mut read) = ws.split();
                // The mic shows audio, Finalize, KeepAlive. The system socket shows silence, KeepAlive.
                let want = if n == 0 { 3 } else { 2 };
                let mut got = Vec::new();
                while let Some(Ok(msg)) = read.next().await {
                    got.push(msg);
                    if got.len() == want {
                        break;
                    }
                }
                let _ = seen_tx.send((n, got));
            });
        }
    });

    let (audio_tx, audio_rx) = tokio::sync::broadcast::channel::<AudioChunk>(16);
    let (txn_tx, _txn_rx) = tokio::sync::broadcast::channel::<TranscriptEvent>(16);
    let mut stt = DeepgramStt::new("fake-key");
    stt.base_url = format!("ws://127.0.0.1:{port}");
    let adapter = tokio::spawn(async move { stt.start(audio_rx, txn_tx, &[]).await.ok() });

    tokio::time::sleep(Duration::from_millis(150)).await;
    audio_tx
        .send(AudioChunk {
            channel: Channel::Mic,
            samples: vec![7; 320],
            ts_ms: 0,
        })
        .unwrap();

    let is_text = |m: &Message, kind: &str| matches!(m, Message::Text(t) if t.contains(&format!("\"{kind}\"")));
    for _ in 0..2 {
        let (n, got) = tokio::time::timeout(Duration::from_secs(12), seen_rx.recv())
            .await
            .expect("both sockets hear from the client within 12 s")
            .unwrap();
        match n {
            0 => {
                assert!(
                    matches!(&got[0], Message::Binary(b) if b.len() == 640 && b[0] == 7),
                    "the mic socket gets its audio"
                );
                assert!(
                    is_text(&got[1], "Finalize"),
                    "then a Finalize, got {:?}",
                    got[1]
                );
                assert!(
                    is_text(&got[2], "KeepAlive"),
                    "then a KeepAlive, got {:?}",
                    got[2]
                );
            }
            _ => {
                assert!(
                    matches!(&got[0], Message::Binary(b) if b.iter().all(|&x| x == 0)),
                    "a socket that never sent audio sends silence first, got {:?}",
                    got[0]
                );
                assert!(
                    is_text(&got[1], "KeepAlive"),
                    "then a KeepAlive, got {:?}",
                    got[1]
                );
            }
        }
    }

    drop(audio_tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), adapter).await;
    server.abort();
}
