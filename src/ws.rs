use std::sync::Arc;
use axum::extract::ws::{Message, WebSocket};
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use tracing::{info, warn};

use crate::pool::Pool;

fn json_msg(value: serde_json::Value) -> Message {
    Message::Text(serde_json::to_string(&value).unwrap().into())
}

/// Handle a WebSocket connection for a playground session.
pub async fn handle_session(mut ws: WebSocket, pool: Arc<Pool>) {
    // Send queue status
    let status = pool.status(None).await;
    let _ = ws.send(json_msg(serde_json::json!({
        "type": "status",
        "active": status.active,
        "max": status.max,
        "queue_length": status.queue_length,
    }))).await;

    if status.active >= status.max {
        let _ = ws.send(json_msg(serde_json::json!({
            "type": "queued",
            "position": status.queue_length + 1,
            "message": format!("You are #{} in queue. Please wait...", status.queue_length + 1),
        }))).await;
    }

    // Acquire a session (may block if queued)
    let session_id = match pool.acquire().await {
        Ok(id) => id,
        Err(e) => {
            let _ = ws.send(json_msg(serde_json::json!({
                "type": "error", "message": e,
            }))).await;
            return;
        }
    };

    info!(session = %session_id, "WebSocket session started");

    let _ = ws.send(json_msg(serde_json::json!({
        "type": "ready",
        "session_id": session_id.to_string(),
        "timeout_secs": pool.remaining_secs(session_id).await.unwrap_or(0),
    }))).await;

    // Use internal channels to bridge async WebSocket with the main loop
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(256);
    let (in_tx, mut in_rx) = mpsc::channel::<Vec<u8>>(256);

    // Task: drain QEMU stdout into out_tx
    let pool_out = pool.clone();
    let sid = session_id;
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(16));
        loop {
            tick.tick().await;
            let mut buf = Vec::new();
            loop {
                let byte = pool_out.with_session(sid, |s| {
                    s.instance.stdout_rx.try_recv().ok()
                }).await;
                match byte {
                    Some(Some(b)) => buf.push(b),
                    _ => break,
                }
            }
            if !buf.is_empty() && out_tx.send(buf).await.is_err() {
                break;
            }
        }
    });

    // Task: forward in_rx bytes to QEMU stdin
    let pool_in = pool.clone();
    let sid = session_id;
    tokio::spawn(async move {
        while let Some(data) = in_rx.recv().await {
            for byte in data {
                let _ = pool_in.with_session(sid, |s| {
                    let tx = s.instance.stdin_tx.clone();
                    tokio::spawn(async move { let _ = tx.send(byte).await; });
                }).await;
            }
            pool_in.touch(sid).await;
        }
    });

    // Main loop: multiplex WebSocket recv + QEMU output send
    loop {
        tokio::select! {
            // QEMU output → WebSocket
            Some(data) = out_rx.recv() => {
                if ws.send(Message::Binary(data.into())).await.is_err() {
                    break;
                }
            }
            // WebSocket input → QEMU
            msg = ws.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text)
                            && cmd.get("type").and_then(|t| t.as_str()) == Some("input")
                            && let Some(data) = cmd.get("data").and_then(|d| d.as_str())
                        {
                            let _ = in_tx.send(data.as_bytes().to_vec()).await;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let _ = in_tx.send(data.to_vec()).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    pool.release(session_id).await;
    warn!(session = %session_id, "WebSocket session ended");
}
