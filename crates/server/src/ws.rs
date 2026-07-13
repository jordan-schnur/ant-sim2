//! The WebSocket boundary. Holds the error surface so `sim` does not have to.
//!
//! Each connection gets two tasks: one that watches the frame channels and
//! writes binary messages, one that reads client commands. Dropping a
//! connection drops only those two tasks — the simulation keeps ticking
//! headless, and reconnecting resumes the view (spec section 9).

use crate::protocol::decode_command;
use crate::sim_thread::{Frame, Handles};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

pub fn router(handles: Handles, web_dist: Option<PathBuf>) -> Router {
    let app = Router::new().route("/ws", get(upgrade)).with_state(handles);

    // Serving the built client is a convenience, not a requirement: in
    // development Vite serves it and proxies /ws here.
    match web_dist {
        Some(dir) if dir.is_dir() => {
            let index = dir.join("index.html");
            app.fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(index)))
        }
        Some(dir) => {
            tracing::warn!(?dir, "web dist not found; serving the socket only");
            app
        }
        None => app,
    }
}

async fn upgrade(ws: WebSocketUpgrade, State(h): State<Handles>) -> Response {
    ws.on_upgrade(move |socket| serve(socket, h))
}

async fn send(tx: &mut SplitSink<WebSocket, Message>, f: Frame) -> bool {
    !f.is_empty() && tx.send(Message::Binary(f.to_vec())).await.is_ok()
}

async fn serve(socket: WebSocket, h: Handles) {
    let (mut tx, mut rx) = socket.split();
    let commands = h.commands.clone();

    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            match msg {
                Message::Binary(b) => match decode_command(&b) {
                    Some(cmd) => {
                        if commands.send(cmd).is_err() {
                            break; // the sim thread is gone
                        }
                    }
                    // Logged and dropped. A malformed command must never take
                    // down the connection, let alone the simulation.
                    None => tracing::warn!(len = b.len(), "dropped malformed command"),
                },
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Text(_) => tracing::warn!("ignoring text message; the protocol is binary"),
            }
        }
    });

    let writer = tokio::spawn(async move {
        // Connect-time state, before waiting on any change. `hello` first: the
        // client sizes its canvas and texture from it.
        //
        // Cloned into an owned Vec inside this scope: a `watch::Ref` guard is
        // not `Send`, and holding one across the `await` below would make the
        // whole task unspawnable.
        let initial: Vec<Frame> = {
            [
                &h.hello, &h.config, &h.terrain, &h.ants, &h.phero, &h.stats,
                &h.colony_meta, &h.chronicle,
            ]
                .iter()
                .map(|rx| rx.borrow().clone())
                .collect()
        };
        for f in initial {
            if !f.is_empty() && !send(&mut tx, f).await {
                return;
            }
        }

        let mut ants = h.ants.clone();
        let mut phero = h.phero.clone();
        let mut terrain = h.terrain.clone();
        let mut stats = h.stats.clone();
        let mut detail = h.detail.clone();
        let mut genome = h.genome.clone();
        let mut config = h.config.clone();
        let mut hello = h.hello.clone();
        let mut colony_meta = h.colony_meta.clone();
        let mut chronicle = h.chronicle.clone();

        loop {
            // `changed()` resolves once per *change*, not once per value. If the
            // sim published three ant frames while this socket was busy writing
            // a 1 MB texture, the client gets only the newest. That is the
            // intended backpressure: a slow client drops frames rather than
            // dragging the simulation down with it.
            let f = tokio::select! {
                Ok(()) = ants.changed()   => ants.borrow_and_update().clone(),
                Ok(()) = phero.changed()  => phero.borrow_and_update().clone(),
                Ok(()) = terrain.changed() => terrain.borrow_and_update().clone(),
                Ok(()) = stats.changed()  => stats.borrow_and_update().clone(),
                Ok(()) = detail.changed() => detail.borrow_and_update().clone(),
                Ok(()) = genome.changed() => genome.borrow_and_update().clone(),
                Ok(()) = config.changed() => config.borrow_and_update().clone(),
                Ok(()) = hello.changed()  => hello.borrow_and_update().clone(),
                Ok(()) = colony_meta.changed() => colony_meta.borrow_and_update().clone(),
                Ok(()) = chronicle.changed() => chronicle.borrow_and_update().clone(),
                else => break,
            };
            if f.is_empty() {
                continue; // nothing selected yet
            }
            if !send(&mut tx, f).await {
                break;
            }
        }
    });

    // Either half finishing means the connection is over. Abort the other so a
    // half-closed socket does not leak a task per reconnect.
    tokio::select! {
        _ = reader => {}
        _ = writer => {}
    }
}
