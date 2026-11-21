use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use mer_agents::HookKind;

use super::*;

fn temp_socket() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("mer").join("mer.sock");
    (dir, path)
}

fn next(rx: &mut UnboundedReceiver<Incoming>) -> Incoming {
    loop {
        if let Ok(incoming) = rx.try_recv() {
            return incoming;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn raw(path: &Path, text: &str) -> String {
    let mut stream = UnixStream::connect(path).expect("connect");
    stream.write_all(text.as_bytes()).expect("write");
    let mut out = String::new();
    stream.read_to_string(&mut out).expect("read");
    out
}

#[test]
fn every_request_and_response_round_trips() {
    let requests = [
        Request::Ping,
        Request::Hook {
            pane: "p".into(),
            agent: "claude".into(),
            kind: HookKind::Waiting,
            session_id: Some("s".into()),
        },
        Request::Ctl(CtlCommand::SpaceList),
        Request::Ctl(CtlCommand::SpaceNew { cwd: Some("/tmp".into()) }),
        Request::Ctl(CtlCommand::SpaceSleep { index: None }),
        Request::Ctl(CtlCommand::SpaceWake { index: 2 }),
        Request::Ctl(CtlCommand::Open { dir: "/x".into() }),
        Request::Ctl(CtlCommand::SpaceDelete { index: 1 }),
        Request::Ctl(CtlCommand::SpaceColor { index: 0, color: Some("violet".into()) }),
        Request::Ctl(CtlCommand::TabSleep { index: 1 }),
        Request::Ctl(CtlCommand::TabColor { index: 1, color: None }),
        Request::Ctl(CtlCommand::Sidebar { collapsed: true }),
        Request::Ctl(CtlCommand::Zoom { change: "in".into() }),
        Request::Ctl(CtlCommand::UiScale { change: "out".into() }),
        Request::Ctl(CtlCommand::SidebarWidth { width: 300 }),
        Request::Ctl(CtlCommand::Search { query: "needle".into() }),
        Request::Ctl(CtlCommand::TabMove { from: 0, to: 2 }),
        Request::Ctl(CtlCommand::TabToSpace { tab: 1, space: 0 }),
        Request::Ctl(CtlCommand::SpaceMove { from: 2, to: 0 }),
        Request::Ctl(CtlCommand::ShowWindow),
        Request::Ctl(CtlCommand::CloseWindow),
        Request::Ctl(CtlCommand::Quit),
        Request::Ctl(CtlCommand::Show { what: "settings".into() }),
    ];
    for request in requests {
        let line = String::from_utf8(protocol::encode_request(&request).unwrap()).unwrap();
        assert!(line.starts_with(r#"{"v":1,"#));
        assert_eq!(protocol::decode_request(&line), Ok(request));
    }
    let info =
        SpaceInfo { index: 0, name: "a".into(), state: "awake".into(), panes: 2, active: true };
    for response in
        [Response::Ok, Response::Pong, Response::Spaces(vec![info]), Response::Error("x".into())]
    {
        let line = String::from_utf8(protocol::encode_response(&response)).unwrap();
        assert_eq!(protocol::decode_response(&line).unwrap(), response);
    }
    for kind in [HookKind::SessionStart, HookKind::Working, HookKind::Waiting, HookKind::Done] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(serde_json::from_str::<HookKind>(&json).unwrap(), kind);
    }
}

#[test]
fn a_different_version_is_rejected() {
    let err = protocol::decode_request(r#"{"v":2,"request":"Ping"}"#).unwrap_err();
    assert_eq!(err, Response::Error("unsupported protocol version".into()));
    let (_dir, path) = temp_socket();
    let (_server, _rx) = Server::bind(&path).unwrap();
    let out = raw(&path, "{\"v\":9,\"request\":\"Ping\"}\n");
    assert!(out.contains("unsupported protocol version"), "{out}");
}

#[test]
fn ping_gets_pong_and_is_running() {
    let (_dir, path) = temp_socket();
    assert!(!Client::is_running(&path));
    let (_server, _rx) = Server::bind(&path).unwrap();
    assert_eq!(Client::send(&path, &Request::Ping).unwrap(), Response::Pong);
    assert!(Client::is_running(&path));
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700);
}

#[test]
fn a_loose_socket_directory_is_tightened() {
    let (_dir, path) = temp_socket();
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (_server, _rx) = Server::bind(&path).unwrap();
    let mode = std::fs::metadata(parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn a_directory_that_is_not_ours_is_left_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = dir.path().join("custom.sock");
    let (_server, _rx) = Server::bind(&path).unwrap();
    let mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "a directory not named `mer` must not be chmodded");
}

#[test]
fn a_hook_is_forwarded_and_answered_ok() {
    let (_dir, path) = temp_socket();
    let (_server, mut rx) = Server::bind(&path).unwrap();
    let request = Request::Hook {
        pane: "pane".into(),
        agent: "codex".into(),
        kind: HookKind::Done,
        session_id: None,
    };
    assert_eq!(Client::send(&path, &request).unwrap(), Response::Ok);
    let incoming = next(&mut rx);
    assert_eq!(incoming.request, request);
    incoming.reply(Response::Error("ignored".into()));
}

#[test]
fn a_ctl_request_waits_for_the_reply() {
    let (_dir, path) = temp_socket();
    let (_server, mut rx) = Server::bind(&path).unwrap();
    let worker = std::thread::spawn(move || {
        let incoming = next(&mut rx);
        assert_eq!(incoming.request, Request::Ctl(CtlCommand::SpaceList));
        incoming.reply(Response::Spaces(vec![]));
    });
    let response = Client::send(&path, &Request::Ctl(CtlCommand::SpaceList)).unwrap();
    assert_eq!(response, Response::Spaces(vec![]));
    worker.join().unwrap();
}

#[test]
fn a_ctl_request_without_reply_times_out() {
    let (_dir, path) = temp_socket();
    let (_server, mut rx) = Server::bind(&path).unwrap();
    let response = Client::send(&path, &Request::Ctl(CtlCommand::SpaceWake { index: 1 })).unwrap();
    assert_eq!(response, Response::Error("timed out".into()));
    assert!(rx.try_recv().is_ok());
}

#[test]
fn a_stale_socket_is_replaced() {
    let (_dir, path) = temp_socket();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    let (_server, _rx) = Server::bind(&path).unwrap();
    assert!(Client::is_running(&path));
}

#[test]
fn a_second_bind_fails_while_the_first_lives() {
    let (_dir, path) = temp_socket();
    let (_server, _rx) = Server::bind(&path).unwrap();
    assert!(matches!(Server::bind(&path), Err(IpcError::AlreadyRunning)));
    assert!(Client::is_running(&path));
}

#[test]
fn the_socket_is_removed_on_drop() {
    let (_dir, path) = temp_socket();
    let (server, _rx) = Server::bind(&path).unwrap();
    assert!(path.exists());
    drop(server);
    assert!(!path.exists());
}

#[test]
fn malformed_input_does_not_kill_the_server() {
    let (_dir, path) = temp_socket();
    let (_server, _rx) = Server::bind(&path).unwrap();
    assert!(raw(&path, "not json\n").contains("Error"));
    assert!(raw(&path, "{\"v\":1,\"request\":\"Nope\"}\n").contains("Error"));
    assert!(raw(&path, "\n").contains("Error"));
    assert!(Client::is_running(&path));
}
