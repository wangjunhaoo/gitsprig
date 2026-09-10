use gitgui_lib::ai::{self, SaveSettings, Settings};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn save_request(endpoint: &str, key: Option<&str>, remove_key: bool) -> SaveSettings {
    SaveSettings {
        endpoint: endpoint.into(),
        model: "custom/model:latest".into(),
        instruction: "中文标题".into(),
        api_key: key.map(String::from),
        remove_key,
    }
}
fn settings(endpoint: &str) -> Settings {
    Settings {
        endpoint: endpoint.into(),
        model: "custom/model:latest".into(),
        instruction: "中文标题".into(),
        has_api_key: false,
    }
}

#[test]
fn endpoint_supports_base_url_and_full_path_and_rejects_embedded_secrets() {
    assert_eq!(
        ai::endpoint(" https://api.example.invalid/v1/ ").unwrap(),
        "https://api.example.invalid/v1/chat/completions"
    );
    assert_eq!(
        ai::endpoint("http://localhost:1234/v1/chat/completions/").unwrap(),
        "http://localhost:1234/v1/chat/completions"
    );
    for invalid in [
        "ftp://localhost/v1",
        "https://user:secret@host/v1",
        "https://host/v1?key=secret",
        "https://host/v1#anchor",
        "host/v1",
    ] {
        assert!(ai::endpoint(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn file_settings_survive_reload_without_exposing_key_to_frontend() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".gitgui");
    let first = ai::save(
        &config,
        save_request(
            "https://first.invalid/v1",
            Some("sensitive-test-token"),
            false,
        ),
    )
    .unwrap();
    assert!(first.has_api_key);
    assert_eq!(
        ai::load_for_generation(&config).unwrap().1.as_deref(),
        Some("sensitive-test-token")
    );
    let stored: Value =
        serde_json::from_slice(&std::fs::read(config.join("ai.json")).unwrap()).unwrap();
    assert_eq!(stored["apiKey"], "sensitive-test-token");
    let public_json = serde_json::to_string(&ai::load(&config).unwrap()).unwrap();
    assert!(!public_json.contains("sensitive-test-token"));
    assert!(!public_json.contains("\"apiKey\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&config).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(config.join("ai.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let retained = ai::save(&config, save_request(&first.endpoint, None, false)).unwrap();
    assert!(retained.has_api_key);
    assert_eq!(
        ai::load_for_generation(&config).unwrap().1.as_deref(),
        Some("sensitive-test-token")
    );
    let second = ai::save(
        &config,
        save_request("https://second.invalid/v1", None, false),
    )
    .unwrap();
    assert!(!second.has_api_key);
    assert!(ai::load_for_generation(&config).unwrap().1.is_none());
    assert!(!std::fs::read_to_string(config.join("ai.json"))
        .unwrap()
        .contains("sensitive-test-token"));
    ai::save(
        &config,
        save_request(&first.endpoint, Some("replacement-token"), false),
    )
    .unwrap();
    let removed = ai::save(&config, save_request(&first.endpoint, None, true)).unwrap();
    assert!(!removed.has_api_key);
    assert!(ai::load_for_generation(&config).unwrap().1.is_none());
    assert!(!std::fs::read_to_string(config.join("ai.json"))
        .unwrap()
        .contains("replacement-token"));
    assert_eq!(std::fs::read_dir(&config).unwrap().count(), 1);
}

#[test]
fn invalid_settings_are_rejected_without_overwriting_or_echoing_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".gitgui");
    assert!(!ai::load(&config).unwrap().has_api_key);
    assert!(!config.exists());
    std::fs::create_dir(&config).unwrap();
    let path = config.join("ai.json");
    let invalid = "invalid-json-secret-sentinel";
    std::fs::write(&path, invalid).unwrap();
    let error = ai::load(&config).err().unwrap();
    assert!(!error.contains(invalid));
    assert!(ai::save(
        &config,
        save_request("https://host.invalid/v1", Some("replacement"), false)
    )
    .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    std::fs::write(&path, "x".repeat(128 * 1024 + 1)).unwrap();
    assert!(ai::load(&config).err().unwrap().contains("128 KB"));
}

#[cfg(unix)]
#[test]
fn local_config_rejects_symlinks_and_repairs_broad_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let linked = dir.path().join("linked");
    std::fs::create_dir(&real).unwrap();
    symlink(&real, &linked).unwrap();
    assert!(ai::save(
        &linked,
        save_request("https://host.invalid/v1", Some("test-key"), false)
    )
    .is_err());
    assert!(ai::load(&linked).is_err());
    let target = dir.path().join("target.json");
    std::fs::write(&target, "{}").unwrap();
    symlink(&target, real.join("ai.json")).unwrap();
    assert!(ai::load(&real).is_err());
    assert!(ai::save(
        &real,
        save_request("https://host.invalid/v1", Some("test-key"), false)
    )
    .is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "{}");
    let config = dir.path().join("config");
    ai::save(
        &config,
        save_request("https://host.invalid/v1", Some("test-key"), false),
    )
    .unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(
        config.join("ai.json"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(ai::load(&config).unwrap().has_api_key);
    assert_eq!(
        std::fs::metadata(&config).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(config.join("ai.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

fn server(status: u16, body: String, delay: Duration) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let start = Instant::now();
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && start.elapsed() < Duration::from_secs(3) =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("本地模拟服务未收到请求：{error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            let text = String::from_utf8_lossy(&bytes);
            if let Some((headers, content)) = text.split_once("\r\n\r\n") {
                let size: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if content.len() >= size {
                    break;
                }
            }
        }
        std::thread::sleep(delay);
        let _ = write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        String::from_utf8(bytes).unwrap()
    });
    (endpoint, thread)
}

#[tokio::test]
async fn sends_custom_model_selected_input_and_optional_bearer() {
    for key in [Some("test-only-key"), None] {
        let (endpoint, thread) = server(
            200,
            json!({"choices":[{"message":{"content":"修复会话刷新"},"finish_reason":"stop"}]})
                .to_string(),
            Duration::ZERO,
        );
        let result = ai::generate(
            &settings(&endpoint),
            key,
            "only selected changes",
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert_eq!(result, "修复会话刷新");
        let request = thread.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert_eq!(
            headers
                .to_lowercase()
                .contains("authorization: bearer test-only-key"),
            key.is_some()
        );
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["model"], "custom/model:latest");
        assert_eq!(body["messages"][1]["content"], "only selected changes");
        assert_eq!(body["stream"], false);
        assert_eq!(body.as_object().unwrap().len(), 3);
    }
}

#[tokio::test]
async fn http_errors_are_actionable_and_do_not_echo_server_secrets() {
    for status in [401, 404, 429, 500, 302] {
        let (endpoint, thread) = server(
            status,
            "server-echoed-sensitive-token".into(),
            Duration::ZERO,
        );
        let error = ai::generate(
            &settings(&endpoint),
            None,
            "selected",
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(3),
        )
        .await
        .unwrap_err();
        assert!(error.contains(&status.to_string()));
        assert!(!error.contains("sensitive-token"));
        thread.join().unwrap();
    }
}

#[tokio::test]
async fn request_timeout_and_cancellation_return_promptly() {
    let (endpoint, thread) = server(200, "{}".into(), Duration::from_millis(350));
    let error = ai::generate(
        &settings(&endpoint),
        None,
        "selected",
        Arc::new(AtomicBool::new(false)),
        Duration::from_millis(100),
    )
    .await
    .unwrap_err();
    assert!(error.contains("超时"));
    thread.join().unwrap();
    let (endpoint, thread) = server(200, "{}".into(), Duration::from_millis(350));
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        flag.store(true, Ordering::SeqCst);
    });
    let start = Instant::now();
    let error = ai::generate(
        &settings(&endpoint),
        None,
        "selected",
        cancel,
        Duration::from_secs(3),
    )
    .await
    .unwrap_err();
    assert!(error.contains("取消"));
    assert!(start.elapsed() < Duration::from_millis(250));
    thread.join().unwrap();
}

#[tokio::test]
async fn oversized_responses_stop_before_parsing() {
    let (endpoint, thread) = server(200, "x".repeat(128 * 1024 + 1), Duration::ZERO);
    assert!(ai::generate(
        &settings(&endpoint),
        None,
        "selected",
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(3)
    )
    .await
    .unwrap_err()
    .contains("128 KB"));
    thread.join().unwrap();
}

#[test]
fn invalid_empty_refused_and_truncated_results_are_rejected() {
    for value in [
        json!({}),
        json!({"choices":[]}),
        json!({"choices":[{"message":{"content":""}}]}),
        json!({"choices":[{"message":{"content":null,"refusal":"refused"}}]}),
        json!({"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}),
    ] {
        assert!(ai::parse_response(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    assert!(ai::parse_response(b"not json").is_err());
    assert!(ai::request_body(
        &settings("https://host.invalid/v1"),
        &"x".repeat(ai::MAX_INPUT + 1)
    )
    .is_err());
}
