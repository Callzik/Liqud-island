//! AI agents in the island (like Lumio): Claude Code and Codex sessions, who
//! works, who waits, and answering them without switching windows.
//!
//! Claude Code reports through hooks: `island.exe --cc-hook` (put into
//! ~/.claude/settings.json by `hooks_install`) forwards every hook event over a
//! localhost socket. For a permission request or a question (AskUserQuestion)
//! the hook waits until the answer is given in the island, or the time runs out
//! and the terminal asks as usual.
//! Codex is only watched: its session logs (~/.codex/sessions/…/rollout-*.jsonl).

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
    #[serde(skip)]
    pids: Vec<u32>,
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
    /// let the terminal ask as usual
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
        .name("codex".into())
        .spawn(codex_loop)
        .expect("failed to start codex thread");
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
    let (pids, cwd) = match lock(&h.sessions).get(session) {
        Some(s) => (s.pids.clone(), s.cwd.clone()),
        None => return false,
    };
    let target = proc::window_for_pids(&pids);
    if target != 0 {
        crate::native::input::activate(target);
        return true;
    }
    // nothing found (Codex is only watched): at least open the project folder
    !cwd.is_empty() && crate::native::shell::open(&cwd)
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
            if !pids.is_empty() {
                s.pids = pids;
            }
        }
        let tool = str_of(ev, "tool_name").to_string();
        let input = ev.get("tool_input").cloned().unwrap_or(Value::Null);
        // the user is looking at this very terminal: let it ask there, no waiting
        let in_front = {
            let fg = proc::window_pid(crate::native::input::foreground());
            fg != 0 && s.pids.contains(&fg)
        };
        match name.as_str() {
            "PermissionRequest" | "PreToolUse" if in_front && (name == "PermissionRequest" || tool == "AskUserQuestion") => {
                s.status = "waiting".into();
                s.action = "Ждёт ответа в терминале".into();
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
                    s.status = "waiting".into();
                    s.action = "Ждёт ответа в терминале".into();
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
            Some(Reply::Pass) => {
                s.status = "waiting".into();
                s.action = "Ждёт ответа в терминале".into();
            }
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

fn title_from(head: &str) -> String {
    let mut first = String::new();
    for line in head.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match str_of(&v, "type") {
            "summary" => {
                let t = str_of(&v, "summary");
                if !t.is_empty() {
                    return short(t, 60);
                }
            }
            "custom-title" | "ai-title" => {
                for k in ["customTitle", "aiTitle", "title"] {
                    let t = str_of(&v, k);
                    if !t.is_empty() {
                        return short(t, 60);
                    }
                }
            }
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
    first
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

fn codex_loop() {
    let root = home().join(".codex").join("sessions");
    let mut files: HashMap<PathBuf, CodexFile> = HashMap::new();
    loop {
        thread::sleep(Duration::from_secs(2));
        let now = now_ms();
        let mut changed = false;
        if root.is_dir() {
            let mut days = Vec::new();
            for y in newest_dirs(&root, 2) {
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
                    if now - mt > 30 * 60 * 1000 && !files.contains_key(&p) {
                        continue;
                    }
                    let f = files.entry(p.clone()).or_default();
                    changed |= codex_read(&p, f, mt, now);
                }
            }
        }
        changed |= forget(now, &mut files);
        if changed {
            emit();
        }
    }
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
            let key = f.key.clone();
            all.entry(key.clone()).or_insert_with(|| Session {
                id: key,
                source: "codex".into(),
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
            s.action = format!("Ждёт подтверждения в терминале? {}", f.last);
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

fn forget(now: i64, files: &mut HashMap<PathBuf, CodexFile>) -> bool {
    let Some(h) = hub() else { return false };
    let mut all = lock(&h.sessions);
    let before = all.len();
    all.retain(|_, s| s.request.is_some() || now - s.updated_ms < FORGET_MS);
    let keys: Vec<String> = all.keys().cloned().collect();
    files.retain(|p, f| keys.contains(&f.key) || now - mtime_ms(p) < 30 * 60 * 1000);
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
    // hooks run through bash on Windows: forward slashes, quoted
    Ok(format!("\"{}\" {HOOK_ARG}", exe.to_string_lossy().replace('\\', "/")))
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
