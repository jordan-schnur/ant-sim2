//! End-to-end over a real socket.
//!
//! The two properties the spec (section 9) demands of this boundary:
//! a malformed client command is logged and dropped, and a dead WebSocket does
//! not stop the simulation. Both are tested here rather than asserted in prose.

use futures_util::{SinkExt, StreamExt};
use server::protocol::*;
use server::{sim_thread, ws};
use sim::config::Config;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

fn small() -> Config {
    Config {
        width: 32,
        height: 32,
        num_colonies: 2,
        initial_ants_per_colony: 4,
        food_patch_count: 2,
        ..Config::default()
    }
}

/// Binds an ephemeral port so tests can run in parallel without colliding.
async fn serve() -> (String, sim_thread::Handles) {
    let handles = sim_thread::spawn(small(), 1, std::env::temp_dir().join("antsim_ws_test.bin"));
    let app = ws::router(handles.clone(), None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{addr}/ws"), handles)
}

async fn connect(
    url: &str,
) -> impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
       + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
       + Unpin {
    let (s, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    s
}

async fn next_binary(
    s: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> Vec<u8> {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), s.next())
            .await
            .expect("timed out waiting for a frame")
            .expect("socket closed")
            .expect("socket error")
        {
            Message::Binary(b) => return b,
            _ => continue,
        }
    }
}

#[tokio::test]
async fn a_new_connection_is_greeted_with_hello_and_the_world_size() {
    let (url, _h) = serve().await;
    let mut s = connect(&url).await;

    let hello = next_binary(&mut s).await;
    assert_eq!(hello[0], TAG_HELLO);
    assert_eq!(hello.len(), 15);
    assert_eq!(u16::from_le_bytes([hello[1], hello[2]]), 32);
    assert_eq!(u16::from_le_bytes([hello[3], hello[4]]), 32);
    assert_eq!(hello[5], 2, "num_colonies");
}

#[tokio::test]
async fn the_connect_burst_carries_config_ants_phero_and_stats() {
    let (url, _h) = serve().await;
    let mut s = connect(&url).await;

    let mut tags = Vec::new();
    for _ in 0..6 {
        tags.push(next_binary(&mut s).await[0]);
    }
    assert_eq!(
        tags,
        vec![
            TAG_HELLO,
            TAG_CONFIG,
            TAG_TERRAIN,
            TAG_ANTS,
            TAG_PHERO,
            TAG_STATS
        ],
        "a client must be able to draw a full picture before the first tick"
    );
}

#[tokio::test]
async fn a_malformed_command_is_dropped_and_the_connection_survives() {
    let (url, _h) = serve().await;
    let mut s = connect(&url).await;
    let _ = next_binary(&mut s).await; // hello

    // Unknown tag, then a truncated payload for a known tag.
    s.send(Message::Binary(vec![0xFF, 0x01, 0x02]))
        .await
        .unwrap();
    s.send(Message::Binary(vec![CMD_RESET, 0x01]))
        .await
        .unwrap();
    s.send(Message::Binary(vec![])).await.unwrap();
    // Text is not part of the protocol either.
    s.send(Message::Text("hello?".into())).await.unwrap();

    // A well-formed command still works afterwards, which proves the reader
    // task did not die on any of the above.
    s.send(Message::Binary(vec![CMD_SET_PAUSED, 0]))
        .await
        .unwrap();
    s.send(Message::Binary(vec![CMD_SET_SPEED, 2]))
        .await
        .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let f = next_binary(&mut s).await;
        if f[0] == TAG_ANTS {
            let tick = u64::from_le_bytes(f[1..9].try_into().unwrap());
            if tick > 0 {
                return; // the sim accepted the command and advanced
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the sim never advanced; a malformed command probably killed the reader"
        );
    }
}

#[tokio::test]
async fn a_dropped_connection_does_not_stop_the_simulation() {
    let (url, h) = serve().await;

    {
        let mut s = connect(&url).await;
        let _ = next_binary(&mut s).await;
        s.send(Message::Binary(vec![CMD_SET_PAUSED, 0]))
            .await
            .unwrap();
        s.send(Message::Binary(vec![CMD_SET_SPEED, 2]))
            .await
            .unwrap();
        // Wait until it is demonstrably running, then hang up.
        loop {
            let f = next_binary(&mut s).await;
            if f[0] == TAG_ANTS && u64::from_le_bytes(f[1..9].try_into().unwrap()) > 0 {
                break;
            }
        }
    } // socket dropped

    let tick_now = || {
        let f = h.ants.borrow().clone();
        u64::from_le_bytes(f[1..9].try_into().unwrap())
    };
    let before = tick_now();

    // Poll to a deadline. A fixed sleep would be racing the tick batch, and in
    // a debug build a batch is slow enough to lose that race.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tick_now() > before {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the sim stalled when the client left (stuck at {before})"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // And a reconnecting client resumes the view mid-run. The authoritative
    // tick is the one on the ant frame; `hello` carries world metadata and a
    // tick *hint* that may lag by up to one stats period.
    let mut s = connect(&url).await;
    let hello = next_binary(&mut s).await;
    assert_eq!(hello[0], TAG_HELLO);
    assert_eq!(u16::from_le_bytes([hello[1], hello[2]]), 32);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let f = next_binary(&mut s).await;
        if f[0] == TAG_ANTS {
            let tick = u64::from_le_bytes(f[1..9].try_into().unwrap());
            assert!(tick > 0, "reconnect showed a world at tick 0");
            return;
        }
        assert!(tokio::time::Instant::now() < deadline, "no ant frame");
    }
}

#[tokio::test]
async fn hello_catches_up_with_the_running_world() {
    // Seeded at tick 0 when the sim thread starts, then refreshed on the stats
    // cadence. A client that connects to a long-running sim must not be told
    // the world just began.
    let (url, _h) = serve().await;
    {
        let mut s = connect(&url).await;
        let _ = next_binary(&mut s).await;
        s.send(Message::Binary(vec![CMD_SET_PAUSED, 0]))
            .await
            .unwrap();
        s.send(Message::Binary(vec![CMD_SET_SPEED, 2]))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    let mut s = connect(&url).await;
    let hello = next_binary(&mut s).await;
    let tick = u64::from_le_bytes(hello[7..15].try_into().unwrap());
    assert!(tick > 0, "hello never caught up with the running world");
}

#[tokio::test]
async fn a_config_command_round_trips_back_as_a_config_frame() {
    let (url, _h) = serve().await;
    let mut s = connect(&url).await;

    let mut cmd = vec![CMD_SET_CONFIG, 10]; // birth_cost
    cmd.extend_from_slice(&12.5f32.to_le_bytes());
    s.send(Message::Binary(cmd)).await.unwrap();

    // The first CONFIG frame is the connect-time one; wait for the echo that
    // carries our value, so the operator's slider reads the server's truth.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let f = next_binary(&mut s).await;
        if f[0] == TAG_CONFIG {
            // field 10 sits at 2 + 10*5, then a tag byte
            let off = 2 + 10 * 5 + 1;
            let v = f32::from_le_bytes(f[off..off + 4].try_into().unwrap());
            if v == 12.5 {
                return;
            }
        }
        assert!(tokio::time::Instant::now() < deadline, "no config echo");
    }
}

#[tokio::test]
async fn selecting_an_ant_yields_a_genome_and_then_detail_frames() {
    let (url, _h) = serve().await;
    let mut s = connect(&url).await;

    // Ant frames tell us where somebody is; pick that spot.
    let mut pos = None;
    while pos.is_none() {
        let f = next_binary(&mut s).await;
        if f[0] == TAG_ANTS {
            let count = u32::from_le_bytes([f[9], f[10], f[11], f[12]]);
            assert!(count > 0);
            let x = u16::from_le_bytes([f[13], f[14]]) as f32 / 128.0;
            let y = u16::from_le_bytes([f[15], f[16]]) as f32 / 128.0;
            pos = Some((x, y));
        }
    }
    let (x, y) = pos.unwrap();

    let mut cmd = vec![CMD_SELECT_AT];
    cmd.extend_from_slice(&x.to_le_bytes());
    cmd.extend_from_slice(&y.to_le_bytes());
    s.send(Message::Binary(cmd)).await.unwrap();

    let (mut saw_genome, mut saw_detail) = (false, false);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !(saw_genome && saw_detail) {
        let f = next_binary(&mut s).await;
        match f[0] {
            TAG_ANT_GENOME => {
                assert_eq!(f.len(), 9 + sim::N_PARAMS * 4);
                saw_genome = true;
            }
            TAG_ANT_DETAIL => {
                assert_eq!(f.len(), ANT_DETAIL_LEN);
                assert_eq!(f[10], 1, "the ant we picked should be alive");
                saw_detail = true;
            }
            _ => {}
        }
        assert!(tokio::time::Instant::now() < deadline, "no genome/detail");
    }
}
