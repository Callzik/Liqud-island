//! AI agents in the island (like Lumio): Claude Code and Codex sessions, who
//! works, who waits, and answering them without switching windows.
//!
//! Claude Code reports through hooks: `island.exe --cc-hook` (put into
//! ~/.claude/settings.json by `hooks_install`) forwards every hook event over a
//! localhost socket. For a permission request or a question (AskUserQuestion)
//! the hook waits until the answer is given in the island, or the time runs out
//! and the terminal asks as usual.
//! Sessions also come from the transcripts themselves (~/.claude/projects,
//! ~/.codex/sessions), so Claude Code in the Claude app, VS Code or Cursor shows
//! up even where hooks don't run; answering needs the hooks.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::native::proc;
use crate::store::now_ms;

/// How long a question / permission waits in the island before the terminal asks.
const ASK_TIMEOUT: Duration = Duration::from_secs(120);
/// The hook's own limit in settings.json (a bit longer than ASK_TIMEOUT).
const HOOK_WAIT_S: u64 = 150;
/// Sessions that went quiet are dropped after this long.
const FORGET_MS: i64 = 45 * 60 * 1000;
const PORT_FILE: &str = "agents.port";
const HOOK_ARG: &str = "--cc-hook";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct QOption {
    pub label: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub multi_select: bool,
    pub options: Vec<QOption>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    /// "permission" | "question"
    pub kind: String,
    pub tool: String,
    /// "Claude хочет выполнить команду"
    pub title: String,
    /// what it is for, e.g. the command's description
    pub summary: String,
    /// the command / file / url itself
    pub detail: String,
    pub questions: Vec<Question>,
    pub started_ms: i64,
    pub timeout_ms: i64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    /// "claude" | "codex"
    pub source: String,
    pub title: String,
    pub project: String,
    pub cwd: String,
    /// working | thinking | permission | question | waiting | done | idle
    pub status: String,
    pub action: String,
    /// share of the context window in use, 0..1
    pub context: f64,
    pub started_ms: i64,
    pub updated_ms: i64,
    pub actions: u32,
    pub request: Option<Request>,
    /// where it runs: "desktop" (Claude / Codex app) | "vscode" | "cursor" | "windsurf" | "jetbrains" | "terminal"
    pub client: String,
    #[serde(skip)]
    pids: Vec<u32>,
    /// last hook event: hooks know better than the transcript
    #[serde(skip)]
    hooked_ms: i64,
    #[serde(skip)]
    transcript: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub sessions: Vec<Session>,
    /// finished tasks today (the board in the office)
    pub done: u32,
    /// tool calls per minute, last 12 minutes (oldest first)
    pub activity: Vec<u32>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub installed: bool,
    /// installed, but for another island.exe (moved / reinstalled elsewhere)
    pub stale: bool,
    pub claude_found: bool,
    pub codex_found: bool,
    pub settings_path: String,
}

enum Reply {
    Allow,
    Deny(String),
    /// answer to AskUserQuestion, already worded for Claude
    Answer(String),
    /// let the terminal / app ask as usual
    Pass,
}

struct Hub {
    app: AppHandle,
    sessions: Mutex<HashMap<String, Session>>,
    waiting: Mutex<HashMap<String, Sender<Reply>>>,
    stats: Mutex<Stats>,
}

#[derive(Default)]
struct Stats {
    day: u64,
    done: u32,
    /// (minute index, count)
    minutes: Vec<(i64, u32)>,
}

static HUB: OnceLock<Hub> = OnceLock::new();

fn hub() -> Option<&'static Hub> {
    HUB.get()
}

// ---------------------------------------------------------------------------
// Public API (commands in main.rs)
// ---------------------------------------------------------------------------

pub fn start(app: AppHandle, dir: PathBuf) {
    if HUB
        .set(Hub {
            app,
            sessions: Mutex::new(HashMap::new()),
            waiting: Mutex::new(HashMap::new()),
            stats: Mutex::new(Stats::default()),
        })
        .is_err()
    {
        return;
    }
    let port_file = dir.join(PORT_FILE);
    thread::Builder::new()
        .name("agents".into())
        .spawn(move || {
            let listener = match TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => {
                    crate::log::warn(&format!("агенты: не открыть порт: {e}"));
                    return;
                }
            };
            if let Ok(addr) = listener.local_addr() {
                let _ = fs::write(&port_file, addr.port().to_string());
            }
            for stream in listener.incoming().flatten() {
                let _ = thread::Builder::new().name("agents-hook".into()).spawn(move || handle(stream));
            }
        })
        .expect("failed to start agents thread");
    thread::Builder::new()
        .name("agents-watch".into())
        .spawn(watch_loop)
        .expect("failed to start agents watch thread");
}

pub fn snapshot() -> Payload {
    let Some(h) = hub() else { return Payload::default() };
    let mut list: Vec<Session> = lock(&h.sessions).values().cloned().collect();
    list.sort_by(|a, b| rank(&a.status).cmp(&rank(&b.status)).then(a.started_ms.cmp(&b.started_ms)));
    let mut st = lock(&h.stats);
    roll_day(&mut st);
    let now_min = now_ms() / 60_000;
    let activity = (0..12)
        .rev()
        .map(|k| st.minutes.iter().find(|(m, _)| *m == now_min - k).map(|(_, n)| *n).unwrap_or(0))
        .collect();
    Payload { sessions: list, done: st.done, activity }
}

/// The island's answer: "allow" | "deny" | "answer" | "chat".
pub fn answer(session: &str, request: &str, decision: &str, text: Option<String>) -> Result<(), String> {
    let h = hub().ok_or("not started")?;
    let tx = lock(&h.waiting).remove(request).ok_or("Вопрос уже закрыт")?;
    let reply = match decision {
        "allow" => Reply::Allow,
        "deny" => Reply::Deny(text.unwrap_or_default()),
        "answer" => Reply::Answer(text.unwrap_or_default()),
        _ => Reply::Pass,
    };
    let chat = matches!(reply, Reply::Pass);
    let _ = tx.send(reply);
    if chat {
        let _ = focus(session);
    }
    Ok(())
}

/// Brings the session's terminal / editor window to the front.
pub fn focus(session: &str) -> bool {
    let Some(h) = hub() else { return false };
    let (pids, cwd, client, source) = match lock(&h.sessions).get(session) {
        Some(s) => (s.pids.clone(), s.cwd.clone(), s.client.clone(), s.source.clone()),
        None => return false,
    };
    let mut target = proc::window_for_pids(&pids);
    if target == 0 {
        // known only from its transcript: the app it runs in
        let exes: &[&str] = match (client.as_str(), source.as_str()) {
            ("desktop", "codex") => &["codex.exe"],
            ("desktop", _) => &["claude.exe"],
            ("vscode", _) => &["code.exe", "code - insiders.exe"],
            ("cursor", _) => &["cursor.exe"],
            ("windsurf", _) => &["windsurf.exe"],
            _ => &["windowsterminal.exe"],
        };
        target = proc::window_for_pids(&proc::pids_by_exe(exes));
    }
    if target != 0 {
        crate::native::input::activate(target);
        return true;
    }
    // nothing found: at least open the project folder
    !cwd.is_empty() && crate::native::shell::open(&cwd)
}

/// "в приложении" / "в VS Code" / "в терминале"
fn place(client: &str) -> &'static str {
    match client {
        "desktop" => "в приложении",
        "vscode" => "в VS Code",
        "cursor" => "в Cursor",
        "windsurf" => "в Windsurf",
        "jetbrains" => "в IDE",
        _ => "в терминале",
    }
}

fn waiting_there(s: &mut Session) {
    s.status = "waiting".into();
    s.action = format!("Ждёт ответа {}", place(&s.client));
}

/// Where the hook's parents live: the Claude app, an editor or a terminal.
fn client_of(pids: &[u32]) -> String {
    let paths: Vec<String> = pids
        .iter()
        .filter_map(|p| crate::native::util::process_exe_path(*p))
        .map(|p| p.to_lowercase().replace('/', "\\"))
        .collect();
    let stem = |p: &String| crate::native::util::exe_stem(p);
    // the app's own Claude Code lives under its folder; the CLI is in .local\bin or npm
    if paths.iter().any(|p| {
        p.contains("anthropicclaude") || (p.contains("\\windowsapps\\") && p.contains("claude")) || p.contains("\\programs\\claude\\")
    }) {
        return "desktop".into();
    }
    for p in &paths {
        match stem(p).as_str() {
            "code" | "code - insiders" | "vscodium" => return "vscode".into(),
            "cursor" => return "cursor".into(),
            "windsurf" => return "windsurf".into(),
            "idea64" | "pycharm64" | "webstorm64" | "rider64" | "goland64" | "clion64" | "phpstorm64" | "rustrover64" => {
                return "jetbrains".into()
            }
            _ => {}
        }
    }
    "terminal".into()
}

/// Claude Code's own entrypoint mark in transcripts, when it has one.
fn client_from_entry(e: &str) -> Option<&'static str> {
    let e = e.to_lowercase();
    if e.is_empty() {
        None
    } else if e.contains("vscode") {
        Some("vscode")
    } else if e.contains("desktop") || e.contains("sdk") || e.contains("app") {
        Some("desktop")
    } else if e == "cli" {
        Some("terminal")
    } else {
        None
    }
}

fn rank(status: &str) -> u8 {
    match status {
        "permission" | "question" | "waiting" => 0,
        "working" | "thinking" => 1,
        _ => 2,
    }
}

fn emit() {
    if let Some(h) = hub() {
        let _ = h.app.emit("agents", snapshot());
    }
}

fn roll_day(st: &mut Stats) {
    let day = (now_ms() / 86_400_000) as u64;
    if st.day != day {
        st.day = day;
        st.done = 0;
    }
}

fn bump_activity() {
    let Some(h) = hub() else { return };
    let mut st = lock(&h.stats);
    let m = now_ms() / 60_000;
    match st.minutes.iter_mut().find(|(k, _)| *k == m) {
        Some((_, n)) => *n += 1,
        None => st.minutes.push((m, 1)),
    }
    st.minutes.retain(|(k, _)| m - *k < 12);
}

fn bump_done() {
    let Some(h) = hub() else { return };
    let mut st = lock(&h.stats);
    roll_day(&mut st);
    st.done += 1;
}

// ---------------------------------------------------------------------------
// Claude Code hook events
// ---------------------------------------------------------------------------

fn handle(stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    {
        let Ok(clone) = stream.try_clone() else { return };
        let mut r = BufReader::new(clone);
        if r.read_line(&mut line).is_err() {
            return;
        }
    }
    let Ok(msg) = serde_json::from_str::<Value>(&line) else { return };
    let out = match on_hook(&msg) {
        Some((req_id, event, rx)) => {
            let started = Instant::now();
            loop {
                match rx.recv_timeout(Duration::from_millis(400)) {
                    Ok(reply) => break after_reply(&msg, &req_id, &event, reply),
                    Err(RecvTimeoutError::Timeout) => {
                        // Esc in the terminal kills the hook: forget the question
                        if peer_closed(&stream) {
                            close_request(&msg, &req_id, None);
                            return;
                        }
                        if started.elapsed() >= ASK_TIMEOUT {
                            break after_reply(&msg, &req_id, &event, Reply::Pass);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break after_reply(&msg, &req_id, &event, Reply::Pass),
                }
            }
        }
        None => String::new(),
    };
    let mut w = &stream;
    let _ = writeln!(w, "{}", json!({ "stdout": out }));
}

fn peer_closed(s: &TcpStream) -> bool {
    if s.set_nonblocking(true).is_err() {
        return true;
    }
    let mut b = [0u8; 1];
    let r = s.peek(&mut b);
    let _ = s.set_nonblocking(false);
    match r {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
    }
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>().trim_end())
    }
}

fn created_ms(path: &str) -> Option<i64> {
    let t = fs::metadata(path).ok()?.created().ok()?;
    Some(t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_millis() as i64)
}

/// Updates the session; for a question / permission returns the channel the
/// island's answer arrives on.
fn on_hook(msg: &Value) -> Option<(String, String, std::sync::mpsc::Receiver<Reply>)> {
    let h = hub()?;
    let ev = msg.get("event")?;
    let sid = str_of(ev, "session_id");
    if sid.is_empty() {
        return None;
    }
    let name = str_of(ev, "hook_event_name").to_string();
    let key = format!("claude:{sid}");
    let now = now_ms();
    let mut wait = None;
    {
        let mut all = lock(&h.sessions);
        if name == "SessionEnd" {
            all.remove(&key);
            drop(all);
            emit();
            return None;
        }
        let transcript = str_of(ev, "transcript_path").to_string();
        let s = all.entry(key.clone()).or_insert_with(|| Session {
            id: key.clone(),
            source: "claude".into(),
            status: "idle".into(),
            action: "Готов к работе".into(),
            started_ms: created_ms(&transcript).unwrap_or(now),
            ..Default::default()
        });
        s.updated_ms = now;
        s.hooked_ms = now;
        let cwd = str_of(ev, "cwd");
        if !cwd.is_empty() {
            s.cwd = cwd.to_string();
            s.project = file_name(cwd.trim_end_matches(['/', '\\']));
        }
        if !transcript.is_empty() {
            s.transcript = transcript;
        }
        if let Some(p) = msg.get("pids").and_then(Value::as_array) {
            let pids: Vec<u32> = p.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
            if !pids.is_empty() && pids != s.pids {
                s.client = client_of(&pids);
                s.pids = pids;
            }
        }
        let tool = str_of(ev, "tool_name").to_string();
        let input = ev.get("tool_input").cloned().unwrap_or(Value::Null);
        // the user is looking at this very terminal / app: let it ask there, no waiting
        let in_front = {
            let fg = proc::window_pid(crate::native::input::foreground());
            fg != 0 && s.pids.contains(&fg)
        };
        match name.as_str() {
            "PermissionRequest" | "PreToolUse" if in_front && (name == "PermissionRequest" || tool == "AskUserQuestion") => {
                waiting_there(s);
                s.request = None;
            }
            "SessionStart" => {
                s.status = "idle".into();
                s.action = "Готов к работе".into();
            }
            "UserPromptSubmit" => {
                if s.title.is_empty() {
                    s.title = short(str_of(ev, "prompt"), 60);
                }
                s.status = "thinking".into();
                s.action = "Думает…".into();
                s.request = None;
            }
            "PreToolUse" if tool == "AskUserQuestion" => {
                let questions: Vec<Question> = input
                    .get("questions")
                    .cloned()
                    .and_then(|q| serde_json::from_value(q).ok())
                    .unwrap_or_default();
                if !questions.is_empty() {
                    let first = questions[0].question.clone();
                    let req = Request {
                        id: crate::store::new_id(),
                        kind: "question".into(),
                        tool: tool.clone(),
                        title: "Claude спрашивает".into(),
                        summary: first.clone(),
                        detail: String::new(),
                        questions,
                        started_ms: now,
                        timeout_ms: ASK_TIMEOUT.as_millis() as i64,
                    };
                    s.status = "question".into();
                    s.action = format!("Спрашивает: {}", short(&first, 60));
                    let (tx, rx) = channel();
                    lock(&h.waiting).insert(req.id.clone(), tx);
                    wait = Some((req.id.clone(), name.clone(), rx));
                    s.request = Some(req);
                }
            }
            "PreToolUse" => {
                s.status = "working".into();
                s.action = describe(&tool, &input);
            }
            "PostToolUse" => {
                s.actions += 1;
                s.status = "thinking".into();
                s.action = "Думает…".into();
                s.request = None;
                bump_activity();
            }
            "PermissionRequest" => {
                let (title, summary, detail) = permission_text(&tool, &input);
                let req = Request {
                    id: crate::store::new_id(),
                    kind: "permission".into(),
                    tool: tool.clone(),
                    title,
                    summary,
                    detail: detail.clone(),
                    questions: Vec::new(),
                    started_ms: now,
                    timeout_ms: ASK_TIMEOUT.as_millis() as i64,
                };
                s.status = "permission".into();
                s.action = format!("Ждёт разрешения: {}", short(detail.trim_start_matches("$ "), 50));
                let (tx, rx) = channel();
                lock(&h.waiting).insert(req.id.clone(), tx);
                wait = Some((req.id.clone(), name.clone(), rx));
                s.request = Some(req);
            }
            "Notification" => {
                let kind = str_of(ev, "notification_type");
                let text = str_of(ev, "message");
                // the terminal shows its own dialog (after the island's time ran out, or "В чате")
                if s.request.is_none() && (kind == "permission_prompt" || kind == "elicitation_dialog" || text.contains("permission")) {
                    waiting_there(s);
                }
            }
            "Stop" => {
                if s.status != "done" {
                    bump_done();
                }
                s.status = "done".into();
                s.action = "Готово — ждёт задачу".into();
                s.request = None;
            }
            _ => {}
        }
        if s.title.is_empty() || name != "PreToolUse" {
            read_transcript(s);
        }
    }
    emit();
    wait
}

/// Turns the island's answer into the hook's stdout and updates the session.
fn after_reply(msg: &Value, req_id: &str, event: &str, reply: Reply) -> String {
    let out = match (&reply, event) {
        (Reply::Allow, "PermissionRequest") => json!({
            "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } }
        })
        .to_string(),
        (Reply::Deny(m), "PermissionRequest") => {
            let m = if m.trim().is_empty() { "Пользователь отказал (Island)." } else { m.as_str() };
            json!({
                "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "deny", "message": m } }
            })
            .to_string()
        }
        (Reply::Answer(text), "PreToolUse") => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": format!(
                    "The user already answered this question in the Island app (no need to ask again). Their answer:\n{text}\nContinue the task using this answer."
                )
            }
        })
        .to_string(),
        _ => String::new(),
    };
    close_request(msg, req_id, Some(&reply));
    out
}

fn close_request(msg: &Value, req_id: &str, reply: Option<&Reply>) {
    let Some(h) = hub() else { return };
    lock(&h.waiting).remove(req_id);
    let sid = msg.get("event").map(|e| str_of(e, "session_id")).unwrap_or("");
    {
        let mut all = lock(&h.sessions);
        let Some(s) = all.get_mut(&format!("claude:{sid}")) else { return };
        if s.request.as_ref().map(|r| r.id.as_str()) != Some(req_id) {
            return;
        }
        let tool = s.request.as_ref().map(|r| r.tool.clone()).unwrap_or_default();
        s.request = None;
        s.updated_ms = now_ms();
        match reply {
            Some(Reply::Allow) => {
                s.status = "working".into();
                let input = msg.get("event").and_then(|e| e.get("tool_input")).cloned().unwrap_or(Value::Null);
                s.action = describe(&tool, &input);
            }
            Some(Reply::Deny(_)) => {
                s.status = "thinking".into();
                s.action = "Отказано — думает дальше".into();
            }
            Some(Reply::Answer(_)) => {
                s.status = "thinking".into();
                s.action = "Получил ответ — работает".into();
            }
            Some(Reply::Pass) => waiting_there(s),
            None => {
                s.status = "thinking".into();
                s.action = "Думает…".into();
            }
        }
    }
    emit();
}

fn describe(tool: &str, input: &Value) -> String {
    let path = str_of(input, "file_path");
    match tool {
        "Bash" | "PowerShell" => {
            let d = str_of(input, "description");
            let cmd = str_of(input, "command");
            if !d.is_empty() {
                short(d, 50)
            } else if cmd.contains("test") {
                "Запускает тесты".into()
            } else {
                format!("Запускает: {}", short(cmd, 40))
            }
        }
        "Write" => format!("Пишет {}", file_name(path)),
        "Edit" | "MultiEdit" => format!("Правит {}", file_name(path)),
        "NotebookEdit" => format!("Правит {}", file_name(str_of(input, "notebook_path"))),
        "Read" => format!("Читает {}", file_name(path)),
        "Grep" | "Glob" | "LS" => "Ищет по коду".into(),
        "WebFetch" | "WebSearch" => "Ищет в интернете".into(),
        "Task" | "Agent" => "Позвал помощника".into(),
        "TodoWrite" => "Планирует".into(),
        "ExitPlanMode" => "Предлагает план".into(),
        t if t.starts_with("mcp__") => format!("Работает с {}", t.split("__").nth(1).unwrap_or("MCP")),
        t => format!("Работает: {t}"),
    }
}

fn permission_text(tool: &str, input: &Value) -> (String, String, String) {
    let path = str_of(input, "file_path");
    match tool {
        "Bash" | "PowerShell" => (
            "Claude хочет выполнить команду".into(),
            short(str_of(input, "description"), 80),
            format!("$ {}", str_of(input, "command")),
        ),
        "Write" => ("Claude хочет создать файл".into(), file_name(path), path.into()),
        "Edit" | "MultiEdit" => ("Claude хочет изменить файл".into(), file_name(path), path.into()),
        "WebFetch" => ("Claude хочет открыть страницу".into(), String::new(), str_of(input, "url").into()),
        "ExitPlanMode" => ("Claude предлагает план".into(), String::new(), short_lines(str_of(input, "plan"), 900)),
        t => {
            let mut d = input.to_string();
            if d.len() > 400 {
                d = format!("{}…", d.chars().take(400).collect::<String>());
            }
            (format!("Claude хочет использовать {t}"), String::new(), d)
        }
    }
}

fn short_lines(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Context use (from the last assistant message) and a title for the session.
fn read_transcript(s: &mut Session) {
    if s.transcript.is_empty() {
        return;
    }
    let Ok(mut f) = fs::File::open(&s.transcript) else { return };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if s.title.is_empty() {
        let mut head = vec![0u8; len.min(256 * 1024) as usize];
        if f.read_exact(&mut head).is_ok() {
            s.title = title_from(&String::from_utf8_lossy(&head));
        }
    }
    let from = len.saturating_sub(192 * 1024);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return;
    }
    let mut tail = Vec::new();
    if f.read_to_end(&mut tail).is_err() {
        return;
    }
    let tail = String::from_utf8_lossy(&tail);
    for line in tail.lines().rev() {
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(u) = v.get("message").and_then(|m| m.get("usage")) else { continue };
        let n = |k: &str| u.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let used = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + n("output_tokens");
        if used <= 0.0 {
            continue;
        }
        let window = if used > 200_000.0 { 1_000_000.0 } else { 200_000.0 };
        s.context = (used / window).min(1.0);
        break;
    }
}

/// The title a line sets: (title, set by the user).
fn title_line(v: &Value) -> Option<(String, bool)> {
    let (keys, custom): (&[&str], bool) = match str_of(v, "type") {
        "custom-title" => (&["customTitle", "title"], true),
        "ai-title" => (&["aiTitle", "title"], false),
        "summary" => (&["summary"], false),
        _ => return None,
    };
    keys.iter().map(|k| str_of(v, k)).find(|t| !t.is_empty()).map(|t| (short(t, 60), custom))
}

/// Session title: the user's name for it, else Claude's (latest), else the first prompt.
fn title_from(head: &str) -> String {
    let mut first = String::new();
    let mut named = String::new();
    let mut custom = false;
    for line in head.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some((t, c)) = title_line(&v) {
            if c || !custom {
                named = t;
                custom |= c;
            }
            continue;
        }
        match str_of(&v, "type") {
            "user" if first.is_empty() => {
                let c = v.get("message").and_then(|m| m.get("content"));
                let text = match c {
                    Some(Value::String(t)) => t.clone(),
                    Some(Value::Array(a)) => a
                        .iter()
                        .find(|p| str_of(p, "type") == "text")
                        .map(|p| str_of(p, "text").to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                if !text.trim().is_empty() && !text.trim_start().starts_with('<') {
                    first = short(&text, 60);
                }
            }
            _ => {}
        }
    }
    if named.is_empty() {
        first
    } else {
        named
    }
}

// ---------------------------------------------------------------------------
// Codex (watched through its session logs) + forgetting quiet sessions
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CodexFile {
    offset: u64,
    key: String,
    pending_call: bool,
    finished: bool,
    last: String,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn mtime_ms(p: &Path) -> i64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Newest entries of a directory (by name: years, months, days sort as text).
fn newest_dirs(p: &Path, n: usize) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(p)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort();
    v.into_iter().rev().take(n).collect()
}

fn watch_loop() {
    let codex_root = home().join(".codex").join("sessions");
    let claude_root = home().join(".claude").join("projects");
    let mut codex: HashMap<PathBuf, CodexFile> = HashMap::new();
    let mut claude: HashMap<PathBuf, ClaudeFile> = HashMap::new();
    let mut dirs: HashMap<PathBuf, i64> = HashMap::new();
    let mut tick = 0u32;
    loop {
        thread::sleep(Duration::from_secs(2));
        let now = now_ms();
        let mut changed = false;
        if codex_root.is_dir() {
            let mut days = Vec::new();
            for y in newest_dirs(&codex_root, 2) {
                for m in newest_dirs(&y, 2) {
                    days.extend(newest_dirs(&m, 2));
                }
            }
            days.sort();
            days.reverse();
            days.truncate(2);
            for d in days {
                let Ok(rd) = fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let mt = mtime_ms(&p);
                    if now - mt > 30 * 60 * 1000 && !codex.contains_key(&p) {
                        continue;
                    }
                    let f = codex.entry(p.clone()).or_default();
                    changed |= codex_read(&p, f, mt, now);
                }
            }
        }
        // Claude Code (CLI, the Claude app, VS Code…): new transcripts every 10 s
        // (only folders whose listing changed), known ones every 2 s
        if tick % 5 == 0 && claude_root.is_dir() {
            claude_discover(&claude_root, &mut claude, &mut dirs, now);
        }
        for (p, f) in claude.iter_mut() {
            let mt = mtime_ms(p);
            if mt != f.mtime || f.watch {
                changed |= claude_read(p, f, mt, now);
            }
        }
        changed |= forget(now, &mut codex, &mut claude);
        tick = tick.wrapping_add(1);
        if changed {
            emit();
        }
    }
}

// ---------------------------------------------------------------------------
// Claude Code transcripts (~/.claude/projects/<folder>/<session>.jsonl)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ClaudeFile {
    offset: u64,
    mtime: i64,
    key: String,
    /// the first read only catches up (no counters for old turns)
    primed: bool,
    /// last meaningful line: "prompt" | "text" | "tool" | "result" | "end"
    last: String,
    /// this Claude Code writes stop_reason: "end_turn" marks the end for sure
    stop_seen: bool,
    custom_title: bool,
    tool: String,
    tool_input: Value,
    /// keep looking although nothing was written (timers below)
    watch: bool,
    finished: bool,
}

fn claude_discover(root: &Path, files: &mut HashMap<PathBuf, ClaudeFile>, dirs: &mut HashMap<PathBuf, i64>, now: i64) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for d in rd.flatten() {
        let dir = d.path();
        if !dir.is_dir() {
            continue;
        }
        let dm = mtime_ms(&dir);
        if dirs.get(&dir) == Some(&dm) {
            continue;
        }
        dirs.insert(dir.clone(), dm);
        let Ok(list) = fs::read_dir(&dir) else { continue };
        for e in list.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("jsonl") || files.contains_key(&p) {
                continue;
            }
            if now - mtime_ms(&p) < 30 * 60 * 1000 {
                files.insert(p, ClaudeFile::default());
            }
        }
    }
}

const ASKS_PERMISSION: [&str; 7] = ["Bash", "PowerShell", "Edit", "MultiEdit", "Write", "NotebookEdit", "WebFetch"];

/// Reads new lines of one transcript; true when the session changed.
fn claude_read(p: &Path, f: &mut ClaudeFile, mtime: i64, now: i64) -> bool {
    let Some(h) = hub() else { return false };
    f.mtime = mtime;
    let Ok(mut file) = fs::File::open(p) else { return false };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len < f.offset {
        f.offset = 0;
    }
    let mut title = String::new();
    let mut skip_partial = false;
    if !f.primed {
        let mut head = vec![0u8; len.min(256 * 1024) as usize];
        if file.read_exact(&mut head).is_ok() {
            title = title_from(&String::from_utf8_lossy(&head));
        }
        // a long session: the end is enough to know where it stands
        if len > 1024 * 1024 {
            f.offset = len - 512 * 1024;
            skip_partial = true;
        }
    }
    let mut buf = Vec::new();
    if len > f.offset && file.seek(SeekFrom::Start(f.offset)).is_ok() {
        let _ = (&mut file).take(len - f.offset).read_to_end(&mut buf);
    }
    let upto = buf.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    f.offset += upto as u64;
    let mut text = String::from_utf8_lossy(&buf[..upto]).into_owned();
    if skip_partial {
        text = text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    }
    let counting = f.primed;
    f.primed = true;

    let mut all = lock(&h.sessions);
    let mut touched = false;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let kind = str_of(&v, "type");
        if f.key.is_empty() {
            let sid = str_of(&v, "sessionId");
            let sid = if sid.is_empty() { p.file_stem().and_then(|x| x.to_str()).unwrap_or("") } else { sid };
            if sid.is_empty() {
                continue;
            }
            f.key = format!("claude:{sid}");
        }
        if let Some((t, c)) = title_line(&v) {
            if c || !f.custom_title {
                title = t;
                f.custom_title |= c;
            }
            continue;
        }
        if kind != "user" && kind != "assistant" {
            continue;
        }
        let key = f.key.clone();
        let s = all.entry(key.clone()).or_insert_with(|| Session {
            id: key,
            source: "claude".into(),
            status: "idle".into(),
            action: "Готов к работе".into(),
            started_ms: created_ms(&p.to_string_lossy()).unwrap_or(now),
            transcript: p.to_string_lossy().into_owned(),
            ..Default::default()
        });
        touched = true;
        let cwd = str_of(&v, "cwd");
        if s.cwd.is_empty() && !cwd.is_empty() {
            s.cwd = cwd.to_string();
            s.project = file_name(cwd.trim_end_matches(['/', '\\']));
        }
        if s.pids.is_empty() {
            if let Some(c) = client_from_entry(str_of(&v, "entrypoint")) {
                s.client = c.into();
            }
        }
        let hooked = now - s.hooked_ms < 10 * 60 * 1000;
        let msg = v.get("message").cloned().unwrap_or(Value::Null);
        let content = msg.get("content").cloned().unwrap_or(Value::Null);
        s.updated_ms = s.updated_ms.max(mtime);
        if kind == "user" {
            let items = content.as_array().cloned().unwrap_or_default();
            if items.iter().any(|i| str_of(i, "type") == "tool_result") {
                f.last = "result".into();
                if !hooked {
                    s.actions += 1;
                    s.status = "thinking".into();
                    s.action = "Думает…".into();
                    if counting {
                        bump_activity();
                    }
                }
                continue;
            }
            let said = match &content {
                Value::String(t) => t.clone(),
                _ => items.iter().find(|i| str_of(i, "type") == "text").map(|i| str_of(i, "text").to_string()).unwrap_or_default(),
            };
            let said = said.trim();
            if said.is_empty() || said.starts_with('<') || v.get("isMeta").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if said.starts_with("[Request interrupted") {
                f.last = "end".into();
                continue;
            }
            if s.title.is_empty() {
                s.title = short(said, 60);
            }
            f.last = "prompt".into();
            f.finished = false;
            if !hooked {
                s.status = "thinking".into();
                s.action = "Думает…".into();
            }
        } else {
            if let Some(u) = msg.get("usage") {
                let n = |k: &str| u.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                let used = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens") + n("output_tokens");
                if used > 0.0 {
                    s.context = (used / if used > 200_000.0 { 1_000_000.0 } else { 200_000.0 }).min(1.0);
                }
            }
            let items = content.as_array().cloned().unwrap_or_default();
            if let Some(t) = items.iter().find(|i| str_of(i, "type") == "tool_use") {
                f.last = "tool".into();
                f.tool = str_of(t, "name").to_string();
                f.tool_input = t.get("input").cloned().unwrap_or(Value::Null);
                if !hooked {
                    s.status = "working".into();
                    s.action = describe(&f.tool, &f.tool_input);
                }
            } else if items.iter().any(|i| str_of(i, "type") == "text") {
                f.last = "text".into();
                if !hooked {
                    s.status = "thinking".into();
                    s.action = "Пишет ответ".into();
                }
            }
            match str_of(&msg, "stop_reason") {
                "" => {}
                "end_turn" => {
                    f.stop_seen = true;
                    f.last = "end".into();
                }
                _ => f.stop_seen = true,
            }
        }
    }
    if !title.is_empty() {
        if let Some(s) = all.get_mut(&f.key) {
            s.title = title;
            touched = true;
        }
    }

    // nothing written for a while: guess what it waits for (hooked sessions know)
    f.watch = false;
    if let Some(s) = all.get_mut(&f.key) {
        let hooked = now - s.hooked_ms < 10 * 60 * 1000;
        let quiet = now - mtime;
        if !hooked {
            match f.last.as_str() {
                // without stop_reason a long pause after text is the best sign
                "end" | "text" if !f.finished && !(f.last == "text" && f.stop_seen) => {
                    if f.last == "end" || quiet > 20_000 {
                        f.finished = true;
                        if counting && quiet < 60_000 {
                            bump_done();
                        }
                        s.status = "done".into();
                        s.action = "Готово — ждёт задачу".into();
                        touched = true;
                    } else {
                        f.watch = true;
                    }
                }
                "tool" if f.tool == "AskUserQuestion" && s.status != "waiting" => {
                    let q = f.tool_input.get("questions").and_then(|q| q.get(0)).map(|q| str_of(q, "question").to_string()).unwrap_or_default();
                    s.status = "waiting".into();
                    s.action = format!("Спрашивает {}: {}", place(&s.client), short(&q, 50));
                    touched = true;
                }
                "tool" if s.status == "working" && (ASKS_PERMISSION.contains(&f.tool.as_str()) || f.tool.starts_with("mcp__")) => {
                    if quiet > 12_000 {
                        s.status = "waiting".into();
                        s.action = format!("Ждёт разрешения {}? {}", place(&s.client), describe(&f.tool, &f.tool_input));
                        touched = true;
                    } else {
                        f.watch = true;
                    }
                }
                "prompt" | "result" if s.status == "thinking" => {
                    if quiet > 180_000 {
                        s.status = "idle".into();
                        s.action = "Ждёт".into();
                        touched = true;
                    } else {
                        f.watch = true;
                    }
                }
                _ => {}
            }
        }
    }
    touched
}

/// Reads new lines of one rollout file; true when the session changed.
fn codex_read(p: &Path, f: &mut CodexFile, mtime: i64, now: i64) -> bool {
    let Some(h) = hub() else { return false };
    let Ok(mut file) = fs::File::open(p) else { return false };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len < f.offset {
        f.offset = 0;
    }
    let mut buf = Vec::new();
    if len > f.offset && file.seek(SeekFrom::Start(f.offset)).is_ok() {
        let _ = (&mut file).take(len - f.offset).read_to_end(&mut buf);
    }
    // only whole lines; a half-written one is read next time
    let upto = buf.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    f.offset += upto as u64;
    let text = String::from_utf8_lossy(&buf[..upto]).into_owned();

    let mut all = lock(&h.sessions);
    let mut touched = false;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let kind = str_of(&v, "type");
        let pl = v.get("payload").cloned().unwrap_or(Value::Null);
        if kind == "session_meta" {
            let id = str_of(&pl, "id");
            if id.is_empty() {
                continue;
            }
            f.key = format!("codex:{id}");
            let cwd = str_of(&pl, "cwd").to_string();
            let origin = str_of(&pl, "originator").to_lowercase();
            let client = if origin.contains("vscode") {
                "vscode"
            } else if origin.contains("desktop") || origin.contains("app") {
                "desktop"
            } else {
                "terminal"
            };
            let key = f.key.clone();
            all.entry(key.clone()).or_insert_with(|| Session {
                id: key,
                source: "codex".into(),
                client: client.into(),
                project: file_name(cwd.trim_end_matches(['/', '\\'])),
                cwd,
                status: "idle".into(),
                action: "Готов к работе".into(),
                started_ms: created_ms(&p.to_string_lossy()).unwrap_or(now),
                ..Default::default()
            });
            touched = true;
            continue;
        }
        let Some(s) = all.get_mut(&f.key) else { continue };
        let sub = str_of(&pl, "type");
        let mut relevant = true;
        match (kind, sub) {
            ("event_msg", "user_message") => {
                let m = str_of(&pl, "message");
                if s.title.is_empty() && !m.trim_start().starts_with('<') {
                    s.title = short(m, 60);
                }
                s.status = "thinking".into();
                s.action = "Думает…".into();
                f.finished = false;
            }
            ("event_msg", "task_started") => {
                s.status = "thinking".into();
                s.action = "Думает…".into();
                f.finished = false;
            }
            ("event_msg", "task_complete") | ("event_msg", "turn_aborted") => {
                if !f.finished && mtime > now - 5 * 60 * 1000 {
                    bump_done();
                }
                f.finished = true;
                f.pending_call = false;
                s.status = "done".into();
                s.action = "Готово — ждёт задачу".into();
            }
            ("event_msg", "token_count") => {
                let info = pl.get("info");
                let used = info
                    .and_then(|i| i.get("last_token_usage"))
                    .and_then(|u| u.get("total_tokens"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let window = info.and_then(|i| i.get("model_context_window")).and_then(Value::as_f64).unwrap_or(272_000.0);
                if used > 0.0 && window > 0.0 {
                    s.context = (used / window).min(1.0);
                }
            }
            ("response_item", "function_call") | ("response_item", "local_shell_call") | ("response_item", "custom_tool_call") => {
                f.pending_call = true;
                s.status = "working".into();
                s.action = codex_action(&pl);
                f.last = s.action.clone();
            }
            ("response_item", "function_call_output") | ("response_item", "custom_tool_call_output") | ("response_item", "local_shell_call_output") => {
                f.pending_call = false;
                s.actions += 1;
                bump_activity();
                s.status = "thinking".into();
                s.action = "Думает…".into();
            }
            _ => relevant = sub == "reasoning" || sub == "agent_reasoning",
        }
        if relevant {
            touched = true;
            s.updated_ms = mtime.max(s.updated_ms);
        }
    }
    // nothing written for a while and no answer to the last step: probably
    // asking for approval in the terminal
    if let Some(s) = all.get_mut(&f.key) {
        let quiet = now - mtime;
        if !f.finished && f.pending_call && quiet > 15_000 && s.status == "working" {
            s.status = "waiting".into();
            s.action = format!("Ждёт подтверждения {}? {}", place(&s.client), f.last);
            touched = true;
        } else if !f.finished && !f.pending_call && quiet > 120_000 && s.status == "thinking" {
            s.status = "idle".into();
            s.action = "Ждёт".into();
            touched = true;
        }
    }
    touched
}

fn codex_action(pl: &Value) -> String {
    let name = str_of(pl, "name");
    if name == "apply_patch" {
        let input = str_of(pl, "input");
        let file = input
            .lines()
            .find_map(|l| l.strip_prefix("*** Update File: ").or_else(|| l.strip_prefix("*** Add File: ")))
            .map(file_name)
            .unwrap_or_default();
        return if file.is_empty() { "Правит файлы".into() } else { format!("Правит {file}") };
    }
    let args: Value = serde_json::from_str(str_of(pl, "arguments")).unwrap_or(Value::Null);
    let cmd = args
        .get("command")
        .or_else(|| pl.get("action").and_then(|a| a.get("command")))
        .map(|c| match c {
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
            Value::String(s) => s.clone(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let cmd = cmd.trim_start_matches("bash -lc ").trim_matches(['\'', '"']).to_string();
    if cmd.is_empty() {
        format!("Работает: {}", if name.is_empty() { "инструмент" } else { name })
    } else if cmd.contains("test") {
        "Запускает тесты".into()
    } else if cmd.starts_with("rg ") || cmd.starts_with("grep ") || cmd.starts_with("find ") || cmd.starts_with("ls") {
        "Ищет по коду".into()
    } else if cmd.starts_with("cat ") || cmd.starts_with("sed -n") || cmd.starts_with("head ") {
        "Читает код".into()
    } else {
        format!("Запускает: {}", short(&cmd, 40))
    }
}

fn forget(now: i64, codex: &mut HashMap<PathBuf, CodexFile>, claude: &mut HashMap<PathBuf, ClaudeFile>) -> bool {
    let Some(h) = hub() else { return false };
    let mut all = lock(&h.sessions);
    let before = all.len();
    all.retain(|_, s| s.request.is_some() || now - s.updated_ms < FORGET_MS);
    codex.retain(|p, f| all.contains_key(&f.key) || now - mtime_ms(p) < 30 * 60 * 1000);
    claude.retain(|p, f| all.contains_key(&f.key) || now - f.mtime.max(mtime_ms(p)) < 30 * 60 * 1000);
    before != all.len()
}

// ---------------------------------------------------------------------------
// Hooks in ~/.claude/settings.json
// ---------------------------------------------------------------------------

const EVENTS: [(&str, u64, bool); 8] = [
    ("SessionStart", 10, false),
    ("SessionEnd", 10, false),
    ("UserPromptSubmit", 10, false),
    ("PreToolUse", HOOK_WAIT_S, true),
    ("PostToolUse", 10, true),
    ("PermissionRequest", HOOK_WAIT_S, true),
    ("Notification", 10, true),
    ("Stop", 10, false),
];

fn claude_settings() -> PathBuf {
    home().join(".claude").join("settings.json")
}

fn hook_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // the CLI runs hooks through Git Bash, the apps may use another shell:
    // forward slashes work everywhere, quotes only when there are spaces
    // (PowerShell would take a quoted path for a string)
    let path = exe.to_string_lossy().replace('\\', "/");
    Ok(if path.contains(' ') { format!("\"{path}\" {HOOK_ARG}") } else { format!("{path} {HOOK_ARG}") })
}

fn is_ours(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hs| hs.iter().any(|h| str_of(h, "command").contains(HOOK_ARG)))
        .unwrap_or(false)
}

fn read_settings(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => Ok(json!({})),
        Ok(t) => serde_json::from_str::<Value>(&t)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| format!("Не удалось прочитать {} — исправьте JSON", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

fn strip_ours(root: &mut Value) {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return };
    for (_, list) in hooks.iter_mut() {
        if let Some(a) = list.as_array_mut() {
            a.retain(|e| !is_ours(e));
        }
    }
    hooks.retain(|_, list| list.as_array().map(|a| !a.is_empty()).unwrap_or(true));
    if hooks.is_empty() {
        if let Some(o) = root.as_object_mut() {
            o.remove("hooks");
        }
    }
}

fn write_settings(path: &Path, root: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let backup = path.with_extension("json.island-backup");
    if path.exists() && !backup.exists() {
        let _ = fs::copy(path, &backup);
    }
    let text = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.island-tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn hooks_status() -> HookStatus {
    let path = claude_settings();
    let cmd = hook_command().unwrap_or_default();
    let mut st = HookStatus {
        claude_found: home().join(".claude").is_dir(),
        codex_found: home().join(".codex").is_dir(),
        settings_path: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    if let Ok(root) = read_settings(&path) {
        let ours: Vec<String> = root
            .get("hooks")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|o| o.values())
            .filter_map(Value::as_array)
            .flatten()
            .filter(|e| is_ours(e))
            .flat_map(|e| e.get("hooks").and_then(Value::as_array).cloned().unwrap_or_default())
            .map(|h| str_of(&h, "command").to_string())
            .filter(|c| c.contains(HOOK_ARG))
            .collect();
        st.installed = !ours.is_empty();
        st.stale = st.installed && ours.iter().any(|c| *c != cmd);
    }
    st
}

pub fn hooks_install() -> Result<HookStatus, String> {
    let path = claude_settings();
    let mut root = read_settings(&path)?;
    strip_ours(&mut root);
    let cmd = hook_command()?;
    let obj = root.as_object_mut().ok_or("settings.json — не объект")?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or("hooks в settings.json — не объект")?;
    for (event, timeout, matcher) in EVENTS {
        let mut entry = json!({ "hooks": [{ "type": "command", "command": cmd, "timeout": timeout }] });
        if matcher {
            entry["matcher"] = json!("*");
        }
        let list = hooks.entry(event).or_insert_with(|| json!([]));
        match list.as_array_mut() {
            Some(a) => a.push(entry),
            None => *list = json!([entry]),
        }
    }
    write_settings(&path, &root)?;
    Ok(hooks_status())
}

pub fn hooks_remove() -> Result<HookStatus, String> {
    let path = claude_settings();
    let mut root = read_settings(&path)?;
    strip_ours(&mut root);
    write_settings(&path, &root)?;
    Ok(hooks_status())
}

/// After an update / move the hooks would point at an old island.exe.
pub fn hooks_refresh() {
    let st = hooks_status();
    if st.stale {
        if let Err(e) = hooks_install() {
            crate::log::warn(&format!("агенты: не обновить хуки: {e}"));
        }
    }
}

// ---------------------------------------------------------------------------
// `island.exe --cc-hook`: runs inside Claude Code for every hook event
// ---------------------------------------------------------------------------

pub fn is_hook_call() -> bool {
    std::env::args().nth(1).as_deref() == Some(HOOK_ARG)
}

/// Forwards the event to the running island and prints its answer. Never
/// fails loudly: without the island Claude Code just goes on as usual.
pub fn hook_main() {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let Ok(event) = serde_json::from_str::<Value>(&input) else { return };
    let Some(dir) = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("dev.callzik.island")) else { return };
    let Some(port) = fs::read_to_string(dir.join(PORT_FILE)).ok().and_then(|p| p.trim().parse::<u16>().ok()) else { return };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else { return };
    let msg = json!({ "pids": proc::ancestors(), "event": event });
    let mut w = &stream;
    if writeln!(w, "{msg}").is_err() {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(HOOK_WAIT_S - 5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let out = serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|v| v.get("stdout").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    if !out.is_empty() {
        let mut so = std::io::stdout();
        let _ = so.write_all(out.as_bytes());
        let _ = so.flush();
    }
}
