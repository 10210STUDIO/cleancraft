//! Connecting CleanCraft to Claude Desktop and Claude Code, on fake Macs.

use cleancraft_core::mcp_setup::{
    self, Answer, ClientKind, ClientState, ConnectError, SERVER, insert_entry, remove_entry,
};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{ActionKind, Settings, executor};

const BIN: &str = "/Applications/CleanCraft.app/Contents/MacOS/CleanCraft";

fn installed() -> FakeMac {
    let mac = FakeMac::new();
    mac.sys_file(BIN, 100);
    mac
}

fn with_desktop(mac: &FakeMac, config: Option<&str>) -> std::path::PathBuf {
    let p = mcp_setup::desktop_config(&mac.env);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    if let Some(c) = config {
        std::fs::write(&p, c).unwrap();
    }
    p
}

fn state(mac: &FakeMac, k: ClientKind) -> ClientState {
    mcp_setup::detect(&mac.env)
        .into_iter()
        .find(|c| c.kind == k)
        .unwrap()
        .state
}

const OTHERS: &str = r#"{
  "mcpServers": {
    "files": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/tester/Desktop"]
    }
  },
  "globalShortcut": "Cmd+Space"
}
"#;

#[test]
fn detection_finds_nothing_on_a_bare_mac() {
    let mac = installed();
    assert_eq!(
        state(&mac, ClientKind::ClaudeDesktop),
        ClientState::NotFound
    );
    assert_eq!(state(&mac, ClientKind::ClaudeCode), ClientState::NotFound);
    assert_eq!(
        mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap_err(),
        ConnectError::NotFound
    );
}

#[test]
fn detection_finds_desktop_by_app_or_folder() {
    let mac = installed();
    std::fs::create_dir_all(mac.env.sys_path("/Applications/Claude.app")).unwrap();
    assert_eq!(state(&mac, ClientKind::ClaudeDesktop), ClientState::Found);
    let mac = installed();
    with_desktop(&mac, Some(OTHERS));
    assert_eq!(state(&mac, ClientKind::ClaudeDesktop), ClientState::Found);
}

#[test]
fn connect_desktop_keeps_other_servers_byte_for_byte() {
    let mac = installed();
    let p = with_desktop(&mac, Some(OTHERS));
    let done = mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap();
    assert!(
        done.message.contains("Quit and reopen Claude Desktop"),
        "{}",
        done.message
    );
    let text = std::fs::read_to_string(&p).unwrap();
    // Everything before our insertion is the original text.
    let cut = OTHERS.find("\n  },\n  \"globalShortcut\"").unwrap();
    assert!(text.starts_with(&OTHERS[..cut]));
    assert!(text.ends_with(&OTHERS[cut..]));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["mcpServers"][SERVER]["command"], BIN);
    assert_eq!(v["mcpServers"][SERVER]["args"][0], "serve");
    assert_eq!(v["mcpServers"]["files"]["command"], "npx");
    assert_eq!(v["globalShortcut"], "Cmd+Space");
    assert_eq!(
        state(&mac, ClientKind::ClaudeDesktop),
        ClientState::Connected
    );
    // Backup of the previous file, in CleanCraft's folder.
    let b = done.backup.unwrap();
    assert!(b.starts_with(mac.env.data_dir().join("mcp-backups")));
    assert_eq!(std::fs::read_to_string(b).unwrap(), OTHERS);
    // Journaled with an Undo command naming only our tool and that file.
    let ActionKind::Toggled { undo_command, .. } = &done.record.kind else {
        panic!("toggled record")
    };
    let cmd = undo_command.clone().unwrap();
    assert_eq!(
        cmd,
        format!(
            "'{BIN}' mcp-setup remove '/Users/tester/Library/Application Support/Claude/claude_desktop_config.json'"
        )
    );
    assert!(
        executor::history(&mac.env)
            .iter()
            .any(|r| r.id == done.record.id)
    );
    // Connecting twice is refused (already connected), nothing written.
    let before = std::fs::read_to_string(&p).unwrap();
    assert!(mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).is_err());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), before);
}

#[test]
fn undo_restores_the_file_exactly() {
    let mac = installed();
    let p = with_desktop(&mac, Some(OTHERS));
    let done = mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap();
    // History's Undo runs the journaled command through env.sys.
    mac.env.fake_sys().unwrap().answer("sh", "");
    executor::undo(&mac.env, &done.record.id).unwrap();
    let ran = mac.env.fake_sys().unwrap().calls();
    assert!(
        ran.iter().any(|c| c.contains("mcp-setup remove")),
        "{ran:?}"
    );
    // What that command does (the CLI entry point of the same binary):
    let code = mcp_setup::cli(
        &mac.env,
        &[
            "remove".into(),
            "/Users/tester/Library/Application Support/Claude/claude_desktop_config.json".into(),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), OTHERS);
    assert_eq!(state(&mac, ClientKind::ClaudeDesktop), ClientState::Found);
}

#[test]
fn new_config_and_created_servers_key_undo_cleanly() {
    let mac = installed();
    let p = with_desktop(&mac, None);
    let done = mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap();
    assert!(done.backup.is_none());
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(v["mcpServers"][SERVER]["args"][0], "serve");

    let orig = "{\n  \"theme\": \"dark\"\n}\n";
    std::fs::write(&p, orig).unwrap();
    let done = mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap();
    let ActionKind::Toggled { undo_command, .. } = &done.record.kind else {
        panic!()
    };
    assert!(undo_command.as_deref().unwrap().ends_with("--drop-empty"));
    assert!(mcp_setup::remove_from_file(&mac.env, &p, true).unwrap());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), orig);
}

#[test]
fn invalid_json_is_never_written() {
    for bad in ["{ \"mcpServers\": { ", "[1, 2]", "{\"mcpServers\": 3}"] {
        let mac = installed();
        let p = with_desktop(&mac, Some(bad));
        let st = state(&mac, ClientKind::ClaudeDesktop);
        let err = mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap_err();
        match err {
            ConnectError::Refused { file, why } => {
                assert_eq!(file, p);
                assert!(!why.is_empty());
            }
            other => panic!("{bad}: {other:?} ({st:?})"),
        }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), bad);
        assert!(!mac.env.data_dir().join("mcp-backups").exists());
        assert!(executor::history(&mac.env).is_empty());
    }
}

#[test]
fn an_existing_cleancraft_entry_elsewhere_is_left_alone() {
    let mac = installed();
    let other = r#"{"mcpServers": {"cleancraft": {"command": "/opt/other/thing", "args": []}}}"#;
    let p = with_desktop(&mac, Some(other));
    assert!(matches!(
        state(&mac, ClientKind::ClaudeDesktop),
        ClientState::Conflict(_)
    ));
    assert!(matches!(
        mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop),
        Err(ConnectError::Refused { .. })
    ));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), other);
}

#[test]
fn not_installed_means_no_change() {
    let mac = FakeMac::new();
    let p = with_desktop(&mac, Some(OTHERS));
    assert_eq!(
        mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap_err(),
        ConnectError::NotInstalled
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), OTHERS);
    // The command-line link counts as installed.
    mac.home_file(".local/bin/cleancraft", 10);
    assert_eq!(
        mcp_setup::server_binary(&mac.env).as_deref(),
        Some("/Users/tester/.local/bin/cleancraft")
    );
}

#[test]
fn claude_code_through_its_command() {
    let mac = installed();
    mac.home_file(".local/bin/claude", 10);
    let f = mac.env.fake_sys().unwrap();
    f.answer("claude mcp list", "files: npx server - Connected\n");
    assert_eq!(state(&mac, ClientKind::ClaudeCode), ClientState::Found);
    f.answer(
        &format!("claude mcp add --scope user cleancraft -- {BIN} serve"),
        "Added stdio MCP server cleancraft\n",
    );
    let done = mcp_setup::connect(&mac.env, ClientKind::ClaudeCode).unwrap();
    assert!(
        f.calls()
            .iter()
            .any(|c| c.ends_with(&format!("mcp add --scope user cleancraft -- {BIN} serve")))
    );
    let ActionKind::Toggled { undo_command, .. } = &done.record.kind else {
        panic!()
    };
    assert_eq!(
        undo_command.as_deref(),
        Some("'/Users/tester/.local/bin/claude' mcp remove --scope user cleancraft")
    );
    assert!(
        !mac.env.home_path(".claude.json").exists(),
        "file untouched"
    );
    f.answer("claude mcp list", "cleancraft: /x serve - Connected\n");
    assert_eq!(state(&mac, ClientKind::ClaudeCode), ClientState::Connected);
}

#[test]
fn claude_code_without_its_command_edits_only_our_key() {
    let mac = installed();
    let orig =
        "{\"numStartups\": 12, \"mcpServers\": {}, \"projects\": {\"/x\": {\"mcpServers\": {}}}}";
    mac.write(&mac.env.home_path(".claude.json"), orig.as_bytes());
    assert_eq!(state(&mac, ClientKind::ClaudeCode), ClientState::Found);
    mcp_setup::connect(&mac.env, ClientKind::ClaudeCode).unwrap();
    let text = std::fs::read_to_string(mac.env.home_path(".claude.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["mcpServers"][SERVER]["command"], BIN);
    assert_eq!(v["projects"]["/x"]["mcpServers"], serde_json::json!({}));
    assert_eq!(v["numStartups"], 12);
    assert!(
        mcp_setup::remove_from_file(&mac.env, &mac.env.home_path(".claude.json"), false).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(mac.env.home_path(".claude.json")).unwrap(),
        orig
    );
}

#[test]
fn removal_refuses_other_files() {
    let mac = installed();
    let p = mac.env.home_path("Documents/x.json");
    mac.write(&p, b"{\"mcpServers\": {\"cleancraft\": {}}}");
    assert!(mcp_setup::remove_from_file(&mac.env, &p, false).is_err());
    assert_ne!(
        mcp_setup::cli(
            &mac.env,
            &["remove".into(), "/Users/tester/Documents/x.json".into()]
        ),
        0
    );
}

#[test]
fn text_edits_round_trip_on_many_shapes() {
    for orig in [
        "{}",
        "{\"a\":1}",
        "{\"mcpServers\":{}}",
        "{\"mcpServers\":{\"x\":{\"command\":\"y\"}}}",
        "{\n\t\"mcpServers\": {\n\t\t\"x\": {\"command\": \"a \\\" } b\"}\n\t},\n\t\"z\": [1, {\"q\": \"}\"}]\n}",
        "{\"mcpServers\":{\"x\":1,\"y\":2},\"w\":null}",
    ] {
        let (new, created) = insert_entry(orig, BIN).unwrap();
        let back = remove_entry(&new, created).unwrap().unwrap();
        assert_eq!(back, orig, "round trip of {orig}");
    }
}

#[test]
fn shown_once_and_not_now_is_remembered() {
    let mac = installed();
    assert!(mcp_setup::welcome_due(&mac.env));
    mcp_setup::remember(&mac.env, Answer::NotNow).unwrap();
    assert!(!mcp_setup::welcome_due(&mac.env));
    // A damaged file never brings the question back.
    std::fs::write(mac.env.data_dir().join("welcome.json"), b"{{").unwrap();
    assert!(!mcp_setup::welcome_due(&mac.env));
}

#[test]
fn connecting_never_changes_agents_may_clean() {
    let mac = installed();
    with_desktop(&mac, Some(OTHERS));
    let before = Settings::load_checked(&mac.env).0;
    assert!(!before.agents_may_clean);
    mcp_setup::connect(&mac.env, ClientKind::ClaudeDesktop).unwrap();
    mcp_setup::remember(&mac.env, Answer::Done).unwrap();
    assert!(!Settings::load_checked(&mac.env).0.agents_may_clean);
    assert!(!Settings::path(&mac.env).exists());
}
