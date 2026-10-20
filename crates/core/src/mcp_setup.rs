//! Connect CleanCraft's MCP server (`cleancraft serve`) to the person's AI apps: Claude
//! Desktop and Claude Code.
//!
//! What this may change, and nothing else:
//! - Claude Desktop's `claude_desktop_config.json`: one key, `mcpServers.cleancraft`.
//! - Claude Code: through its own command (`claude mcp add --scope user cleancraft`), or,
//!   without the command, the same one key in `~/.claude.json`.
//!
//! A settings file is parsed first; when it is not a valid JSON object nothing is written.
//! The new text is the old text with our entry inserted (everything else byte for byte), it
//! is parsed again and must equal the old content plus our entry, the old file is copied to
//! `<data>/mcp-backups/` with a timestamp, and the new one replaces it with one rename. An
//! existing entry named `cleancraft` is never overwritten, and other servers are never
//! touched. Each change is journaled with an Undo that removes our entry the same careful way.
//!
//! Connecting does not let agents clean: they can scan and preview only until the person turns
//! on "AI agents may clean" (`Settings::agents_may_clean`), and even then the MCP server runs
//! only a plan whose digest was previewed.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::env::Env;
use crate::executor::{self, ActionKind, ActionRecord};
use crate::sys::shell_quote;

/// Our server's name in every config.
pub const SERVER: &str = "cleancraft";

/// Where install-app.sh puts the app's binary, and its command-line link.
pub const APP_BINARY: &str = "/Applications/CleanCraft.app/Contents/MacOS/CleanCraft";
pub const LINK_BINARY: &str = ".local/bin/cleancraft";

pub const DESKTOP_CONFIG: &str = "Library/Application Support/Claude/claude_desktop_config.json";
pub const CODE_CONFIG: &str = ".claude.json";

/// Where the `claude` command is usually installed (home-relative or absolute).
pub const CLAUDE_CLI_HOME: &[&str] = &[".local/bin/claude", ".claude/local/claude"];
pub const CLAUDE_CLI_SYS: &[&str] = &["/opt/homebrew/bin/claude", "/usr/local/bin/claude"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    ClaudeDesktop,
    ClaudeCode,
}

impl ClientKind {
    pub const ALL: [ClientKind; 2] = [ClientKind::ClaudeDesktop, ClientKind::ClaudeCode];

    pub fn title(self) -> &'static str {
        match self {
            ClientKind::ClaudeDesktop => "Claude Desktop",
            ClientKind::ClaudeCode => "Claude Code",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientState {
    NotFound,
    /// Installed, CleanCraft not connected yet.
    Found,
    Connected,
    /// Its settings file is not valid JSON (or not an object): CleanCraft will not write it.
    Invalid(String),
    /// It already has a server named "cleancraft" that runs something else: left as is.
    Conflict(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub kind: ClientKind,
    pub state: ClientState,
    /// The settings file (real path in the env).
    pub config: PathBuf,
    /// Claude Code's command, when found.
    pub cli: Option<PathBuf>,
}

/// The binary agents should run, as macOS names it: the installed app, else the
/// command-line link. None when CleanCraft is not installed (a development build).
pub fn server_binary(env: &Env) -> Option<String> {
    let app = env.sys_path(APP_BINARY);
    if app.is_file() {
        return Some(env.display(&app));
    }
    let link = env.home_path(LINK_BINARY);
    if link.exists() {
        return Some(env.display(&link));
    }
    None
}

pub fn desktop_config(env: &Env) -> PathBuf {
    env.home_path(DESKTOP_CONFIG)
}

pub fn code_config(env: &Env) -> PathBuf {
    env.home_path(CODE_CONFIG)
}

/// The `claude` command, from its usual places only.
pub fn claude_cli(env: &Env) -> Option<PathBuf> {
    CLAUDE_CLI_HOME
        .iter()
        .map(|p| env.home_path(p))
        .chain(CLAUDE_CLI_SYS.iter().map(|p| env.sys_path(p)))
        .find(|p| p.is_file())
}

fn desktop_installed(env: &Env) -> bool {
    env.sys_path("/Applications/Claude.app").exists()
        || env.home_path("Applications/Claude.app").exists()
        || desktop_config(env).parent().is_some_and(|d| d.is_dir())
}

/// State of our entry in a JSON settings file.
fn file_state(path: &Path, binary: Option<&str>) -> ClientState {
    // A link (a dotfiles folder): replacing it would cut the link, so it is left alone.
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return ClientState::Invalid(
            "it is a link to another file, which CleanCraft does not replace".into(),
        );
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return ClientState::Found,
        Err(e) => return ClientState::Invalid(format!("it could not be read: {e}")),
    };
    if text.trim().is_empty() {
        return ClientState::Found;
    }
    let v = match parse_object(&text) {
        Ok(v) => v,
        Err(e) => return ClientState::Invalid(e),
    };
    match v.get("mcpServers").and_then(|s| s.get(SERVER)) {
        None => ClientState::Found,
        Some(entry) => {
            let cmd = entry.get("command").and_then(|c| c.as_str()).unwrap_or("");
            if binary.is_none_or(|b| b == cmd)
                || cmd.ends_with("/cleancraft")
                || cmd.ends_with("/CleanCraft")
            {
                ClientState::Connected
            } else {
                ClientState::Conflict(format!(
                    "it already has a server named \"{SERVER}\" that runs {cmd}"
                ))
            }
        }
    }
}

/// Which AI apps are installed and whether CleanCraft is connected to them.
pub fn detect(env: &Env) -> Vec<Client> {
    let bin = server_binary(env);
    let mut out = Vec::new();
    let dc = desktop_config(env);
    out.push(Client {
        kind: ClientKind::ClaudeDesktop,
        state: if desktop_installed(env) {
            file_state(&dc, bin.as_deref())
        } else {
            ClientState::NotFound
        },
        config: dc,
        cli: None,
    });
    let cli = claude_cli(env);
    let cc = code_config(env);
    let state = match &cli {
        Some(c) => {
            let shown = env.display(c);
            match env.sys.run(&shown, &["mcp", "list"]) {
                Ok(o) if o.ok() => {
                    if listed(&o.stdout) {
                        ClientState::Connected
                    } else {
                        ClientState::Found
                    }
                }
                // The command is there but did not answer: the file decides.
                _ if cc.exists() => file_state(&cc, bin.as_deref()),
                _ => ClientState::Found,
            }
        }
        None if cc.exists() => file_state(&cc, bin.as_deref()),
        None => ClientState::NotFound,
    };
    out.push(Client {
        kind: ClientKind::ClaudeCode,
        state,
        config: cc,
        cli,
    });
    out
}

/// The exact change, in words and code, for the person to see before choosing Connect.
pub fn planned_change(env: &Env, c: &Client) -> Option<String> {
    let bin = server_binary(env)?;
    Some(match (c.kind, &c.cli) {
        (ClientKind::ClaudeCode, Some(cli)) => format!(
            "Runs: {} mcp add --scope user {SERVER} -- {bin} serve",
            env.display(cli)
        ),
        _ => format!(
            "Adds to {} under mcpServers: {}",
            env.display(&c.config),
            entry_json(&bin)
        ),
    })
}

/// `claude mcp list` names our server ("cleancraft: /path serve - ✓ Connected").
pub fn listed(stdout: &str) -> bool {
    stdout
        .lines()
        .any(|l| l.trim_start().starts_with(&format!("{SERVER}:")))
}

// ---------------------------------------------------------------------------------------------
// JSON text editing: our one key, everything else byte for byte.

fn parse_object(text: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err("it does not hold a JSON object".into()),
        Err(e) => Err(format!("it is not valid JSON ({e})")),
    }
}

struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// `i` at an opening quote: the index after the closing one.
fn skip_string(b: &[u8], mut i: usize) -> Option<usize> {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => skip_string(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = skip_string(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') && !b[j].is_ascii_whitespace()
            {
                j += 1;
            }
            Some(j)
        }
    }
}

/// Members of the object whose `{` is at `open`, and the index of its `}`.
fn members(text: &str, open: usize) -> Option<(Vec<Member>, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = skip_ws(b, open + 1);
    if b.get(i) == Some(&b'}') {
        return Some((out, i));
    }
    loop {
        if b.get(i) != Some(&b'"') {
            return None;
        }
        let key_end = skip_string(b, i)?;
        let key: String = serde_json::from_str(&text[i..key_end]).ok()?;
        let colon = skip_ws(b, key_end);
        if b.get(colon) != Some(&b':') {
            return None;
        }
        let vs = skip_ws(b, colon + 1);
        let ve = skip_value(b, vs)?;
        out.push(Member {
            key,
            key_start: i,
            value_start: vs,
            value_end: ve,
        });
        let n = skip_ws(b, ve);
        match b.get(n) {
            Some(b',') => i = skip_ws(b, n + 1),
            Some(b'}') => return Some((out, n)),
            _ => return None,
        }
    }
}

/// The whitespace a member line starts with ("  "), from the member before `at`.
fn indent_before(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let lead = &text[line_start..at];
    if lead.chars().all(|c| c == ' ' || c == '\t') {
        lead.to_string()
    } else {
        String::new()
    }
}

/// The exact entry CleanCraft adds, as written into the file (shown before connecting).
pub fn entry_json(binary: &str) -> String {
    format!(
        "{}: {{\"command\": {}, \"args\": [\"serve\"]}}",
        serde_json::to_string(SERVER).unwrap_or_default(),
        serde_json::to_string(binary).unwrap_or_default()
    )
}

/// Insert our entry into `text` (a JSON object; empty text counts as `{}`). Returns the new
/// text and whether the `mcpServers` key was created. Errors say why nothing was written.
pub fn insert_entry(text: &str, binary: &str) -> Result<(String, bool), String> {
    let text_in = if text.trim().is_empty() { "{}\n" } else { text };
    let before = parse_object(text_in)?;
    if before.get("mcpServers").is_some_and(|s| !s.is_object()) {
        return Err("its \"mcpServers\" is not an object".into());
    }
    if before
        .get("mcpServers")
        .and_then(|s| s.get(SERVER))
        .is_some()
    {
        return Err(format!("it already has a server named \"{SERVER}\""));
    }
    let open = text_in.find('{').ok_or("it does not hold a JSON object")?;
    let (top, close) = members(text_in, open).ok_or("it could not be read safely")?;
    let entry = entry_json(binary);
    let (new, created) = match top.iter().find(|m| m.key == "mcpServers") {
        Some(m) => {
            let (inner, iclose) =
                members(text_in, m.value_start).ok_or("it could not be read safely")?;
            let new = match inner.last() {
                Some(last) => {
                    let ind = indent_before(text_in, last.key_start);
                    let sep = if ind.is_empty() {
                        " ".to_string()
                    } else {
                        format!("\n{ind}")
                    };
                    format!(
                        "{},{sep}{entry}{}",
                        &text_in[..last.value_end],
                        &text_in[last.value_end..]
                    )
                }
                None => format!("{}{entry}{}", &text_in[..iclose], &text_in[iclose..]),
            };
            (new, false)
        }
        None => {
            let servers = format!("\"mcpServers\": {{{entry}}}");
            let new = match top.last() {
                Some(last) => {
                    let ind = indent_before(text_in, last.key_start);
                    let sep = if ind.is_empty() {
                        " ".to_string()
                    } else {
                        format!("\n{ind}")
                    };
                    format!(
                        "{},{sep}{servers}{}",
                        &text_in[..last.value_end],
                        &text_in[last.value_end..]
                    )
                }
                None => format!("{}{servers}{}", &text_in[..close], &text_in[close..]),
            };
            (new, true)
        }
    };
    // The result must be the old content plus our entry, nothing else.
    let mut after = parse_object(&new).map_err(|e| format!("the edit went wrong: {e}"))?;
    strip_ours(&mut after, created);
    if after != before {
        return Err("the edit would have changed something else, so nothing was written".into());
    }
    Ok((new, created))
}

fn strip_ours(v: &mut Value, drop_servers: bool) {
    if let Some(obj) = v.as_object_mut()
        && let Some(Value::Object(s)) = obj.get_mut("mcpServers")
    {
        s.remove(SERVER);
        if drop_servers && s.is_empty() {
            obj.remove("mcpServers");
        }
    }
}

/// Remove our entry (and `mcpServers` when `drop_servers` and it is left empty) from `text`.
/// Ok(None) when there is no entry of ours.
pub fn remove_entry(text: &str, drop_servers: bool) -> Result<Option<String>, String> {
    let before = parse_object(text)?;
    if before
        .get("mcpServers")
        .and_then(|s| s.get(SERVER))
        .is_none()
    {
        return Ok(None);
    }
    let open = text.find('{').ok_or("it does not hold a JSON object")?;
    let (top, tclose) = members(text, open).ok_or("it could not be read safely")?;
    let si = top
        .iter()
        .position(|m| m.key == "mcpServers")
        .ok_or("it could not be read safely")?;
    let servers = &top[si];
    let (inner, iclose) =
        members(text, servers.value_start).ok_or("it could not be read safely")?;
    let k = inner
        .iter()
        .position(|m| m.key == SERVER)
        .ok_or("it could not be read safely")?;
    // The span that removes member `k` of `list` (with its comma) from an object whose
    // braces are at `open` and `close`.
    let cut = |list: &[Member], k: usize, open: usize, close: usize| -> (usize, usize) {
        if list.len() == 1 {
            (open + 1, close)
        } else if k + 1 == list.len() {
            (list[k - 1].value_end, list[k].value_end)
        } else {
            (list[k].key_start, list[k + 1].key_start)
        }
    };
    let (a, b) = if inner.len() == 1 && drop_servers {
        cut(&top, si, open, tclose)
    } else {
        cut(&inner, k, servers.value_start, iclose)
    };
    let new = format!("{}{}", &text[..a], &text[b..]);
    let after = parse_object(&new).map_err(|e| format!("the edit went wrong: {e}"))?;
    let mut expect = before;
    strip_ours(&mut expect, drop_servers);
    if after != expect {
        return Err("the edit would have changed something else, so nothing was written".into());
    }
    Ok(Some(new))
}

// ---------------------------------------------------------------------------------------------
// Writing

fn backup(env: &Env, path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let dir = env.data_dir().join("mcp-backups");
    std::fs::create_dir_all(&dir)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().trim_start_matches('.').to_string())
        .unwrap_or_else(|| "config".into());
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%3f");
    let to = dir.join(format!("{stem}-{ts}.json"));
    std::fs::copy(path, &to)?;
    // It may hold other servers' keys: readable by the person only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(Some(to))
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".cleancraft-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    // Keep the file's permissions.
    if let Ok(md) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, md.permissions());
    }
    std::fs::rename(&tmp, path)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// CleanCraft is not installed in Applications (a development build).
    NotInstalled,
    NotFound,
    /// The settings file was left alone; the text says why. The path can be opened.
    Refused {
        why: String,
        file: PathBuf,
    },
    Failed(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::NotInstalled => write!(
                f,
                "CleanCraft is not installed in Applications, so there is no fixed path for AI apps to start it."
            ),
            ConnectError::NotFound => write!(f, "That app was not found on this Mac."),
            ConnectError::Refused { why, .. } => {
                write!(f, "Its settings file was not changed: {why}.")
            }
            ConnectError::Failed(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Connected {
    pub kind: ClientKind,
    pub record: ActionRecord,
    /// The copy of the settings file before the change, when there was one.
    pub backup: Option<PathBuf>,
    /// What to tell the person.
    pub message: String,
}

fn record(kind: ActionKind) -> ActionRecord {
    let now = chrono::Utc::now();
    let plan_id = format!("mcp-setup-{}", now.timestamp_nanos_opt().unwrap_or(0));
    ActionRecord {
        id: format!("{plan_id}#0"),
        plan_id,
        feature: None,
        time: now.timestamp(),
        size: 0,
        kind,
        undone: None,
    }
}

/// Add CleanCraft to one AI app. Never changes `agents_may_clean`.
pub fn connect(env: &Env, kind: ClientKind) -> Result<Connected, ConnectError> {
    let bin = server_binary(env).ok_or(ConnectError::NotInstalled)?;
    let client = detect(env)
        .into_iter()
        .find(|c| c.kind == kind)
        .ok_or(ConnectError::NotFound)?;
    match &client.state {
        ClientState::NotFound => return Err(ConnectError::NotFound),
        ClientState::Invalid(why) | ClientState::Conflict(why) => {
            return Err(ConnectError::Refused {
                why: why.clone(),
                file: client.config.clone(),
            });
        }
        _ => {}
    }
    let restart = match kind {
        ClientKind::ClaudeDesktop => " Quit and reopen Claude Desktop to use it.",
        ClientKind::ClaudeCode => " New Claude Code sessions can use it.",
    };
    if client.state == ClientState::Connected {
        return Err(ConnectError::Failed(format!(
            "{} is already connected to CleanCraft.",
            kind.title()
        )));
    }
    if kind == ClientKind::ClaudeCode
        && let Some(cli) = &client.cli
    {
        let shown = env.display(cli);
        let out = env
            .sys
            .run(
                &shown,
                &["mcp", "add", "--scope", "user", SERVER, "--", &bin, "serve"],
            )
            .map_err(|e| ConnectError::Failed(format!("claude could not run: {e}")))?;
        if !out.ok() {
            return Err(ConnectError::Failed(format!(
                "claude mcp add did not work: {}",
                out.stderr.lines().next().unwrap_or("").trim()
            )));
        }
        let rec = record(ActionKind::Toggled {
            what: "Connected CleanCraft to Claude Code".into(),
            undo_command: Some(format!(
                "{} mcp remove --scope user {SERVER}",
                shell_quote(&shown)
            )),
        });
        executor::append_journal(env, &rec).map_err(|e| {
            ConnectError::Failed(format!(
                "Connected, but the action log could not be written: {e}"
            ))
        })?;
        return Ok(Connected {
            kind,
            record: rec,
            backup: None,
            message: format!("Claude Code is connected to CleanCraft.{restart}"),
        });
    }
    let path = client.config.clone();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(ConnectError::Refused {
                why: format!("it could not be read: {e}"),
                file: path,
            });
        }
    };
    let (new, created) = insert_entry(&text, &bin).map_err(|why| ConnectError::Refused {
        why,
        file: path.clone(),
    })?;
    let saved = backup(env, &path).map_err(|e| {
        ConnectError::Failed(format!(
            "No copy of the settings file could be kept, so it was not changed: {e}"
        ))
    })?;
    write_atomic(&path, &new).map_err(|e| {
        ConnectError::Failed(format!("The settings file could not be written: {e}"))
    })?;
    let shown = env.display(&path);
    let rec = record(ActionKind::Toggled {
        what: format!("Connected CleanCraft to {}", kind.title()),
        undo_command: Some(format!(
            "{} mcp-setup remove {}{}",
            shell_quote(&bin),
            shell_quote(&shown),
            if created { " --drop-empty" } else { "" }
        )),
    });
    executor::append_journal(env, &rec).map_err(|e| {
        ConnectError::Failed(format!(
            "Connected, but the action log could not be written: {e}"
        ))
    })?;
    Ok(Connected {
        kind,
        record: rec,
        backup: saved,
        message: format!("{} is connected to CleanCraft.{restart}", kind.title()),
    })
}

/// Remove our entry from one of the two known settings files (the journal's Undo runs this
/// through `cleancraft mcp-setup remove <file>`). Any other file is refused.
pub fn remove_from_file(env: &Env, file: &Path, drop_servers: bool) -> Result<bool, String> {
    let known = [desktop_config(env), code_config(env)];
    if !known.iter().any(|k| k == file) {
        return Err(format!(
            "{} is not a settings file CleanCraft manages",
            file.display()
        ));
    }
    if std::fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("it is a link to another file, which CleanCraft does not replace".into());
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("could not read it: {e}"))?;
    let Some(new) = remove_entry(&text, drop_servers)? else {
        return Ok(false);
    };
    backup(env, file).map_err(|e| format!("no copy could be kept: {e}"))?;
    write_atomic(file, &new).map_err(|e| format!("could not write it: {e}"))?;
    Ok(true)
}

/// `cleancraft mcp-setup remove <file> [--drop-empty]`. Returns the exit code.
pub fn cli(env: &Env, args: &[String]) -> i32 {
    match args {
        [cmd, file, rest @ ..] if cmd == "remove" => {
            // The journal names the file as macOS shows it (mapped into a fake Mac in tests).
            let real = env.sys_path(file);
            let drop = rest.iter().any(|a| a == "--drop-empty");
            match remove_from_file(env, &real, drop) {
                Ok(true) => {
                    println!("Removed CleanCraft from {}.", file);
                    0
                }
                Ok(false) => {
                    println!("CleanCraft was not in {}.", file);
                    0
                }
                Err(e) => {
                    eprintln!("Nothing was changed: {e}.");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: cleancraft mcp-setup remove <settings file> [--drop-empty]");
            2
        }
    }
}

// ---------------------------------------------------------------------------------------------
// First launch

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    NotNow,
    Done,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    #[serde(default)]
    pub ai_answer: Option<Answer>,
    #[serde(default)]
    pub at: i64,
}

fn welcome_path(env: &Env) -> PathBuf {
    env.data_dir().join("welcome.json")
}

/// Whether the first-launch question is still to be asked. A damaged file counts as asked
/// (never nag twice because of a bad file).
pub fn welcome_due(env: &Env) -> bool {
    match std::fs::read(welcome_path(env)) {
        Ok(b) => serde_json::from_slice::<Welcome>(&b).is_ok_and(|w| w.ai_answer.is_none()),
        Err(e) => e.kind() == io::ErrorKind::NotFound,
    }
}

/// Remember the answer; the question is not asked at launch again.
pub fn remember(env: &Env, answer: Answer) -> io::Result<()> {
    let w = Welcome {
        ai_answer: Some(answer),
        at: chrono::Utc::now().timestamp(),
    };
    let p = welcome_path(env);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_vec(&w).map_err(io::Error::other)?)
}
