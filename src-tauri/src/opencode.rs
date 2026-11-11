//! opencode sessions, watched through its storage folder
//! (`~/.local/share/opencode/storage`: session/<project>/<id>.json,
//! message/<session>/<msg>.json, part/<msg>/<part>.json). Read-only, like Codex:
//! approvals stay in the terminal, a long silence on a tool is shown as "waiting".

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const RECENT_MS: i64 = 30 * 60 * 1000;

#[derive(Default)]
pub struct OcFile {
    pub key: String,
    pub id: String,
    pub mtime: i64,
    last_tool: String,
    finished_msg: String,
}

pub fn root() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::home().join(".local").join("share"));
    data.join("opencode").join("storage")
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}

fn sorted_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn s<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    let mut cur = v;
    for k in path {
        match cur.get(k) {
            Some(x) => cur = x,
            None => return "",
        }
    }
    cur.as_str().unwrap_or("")
}

fn n(v: &Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for k in path {
        match cur.get(k) {
            Some(x) => cur = x,
            None => return 0.0,
        }
    }
    cur.as_f64().unwrap_or(0.0)
}

/// New and renamed sessions (every 10 s): session files touched in the last half hour.
pub fn discover(root: &Path, files: &mut HashMap<String, OcFile>, now: i64) -> bool {
    let mut changed = false;
    let Ok(projects) = fs::read_dir(root.join("session")) else { return false };
    for p in projects.flatten() {
        for f in sorted_files(&p.path()) {
            let mt = super::mtime_ms(&f);
            let id = f.file_stem().and_then(|x| x.to_str()).unwrap_or_default().to_string();
            if id.is_empty() || (now - mt > RECENT_MS && !files.contains_key(&id)) {
                continue;
            }
            let Some(v) = read_json(&f) else { continue };
            // sub-agents (task tool) are part of their parent session
            if !s(&v, &["parentID"]).is_empty() {
                continue;
            }
            let key = format!("opencode:{id}");
            let title = s(&v, &["title"]).to_string();
            let cwd = s(&v, &["directory"]).to_string();
            let created = n(&v, &["time", "created"]) as i64;
            let entry = files.entry(id.clone()).or_insert_with(|| OcFile { key: key.clone(), id: id.clone(), ..Default::default() });
            entry.mtime = entry.mtime.max(mt);
            changed |= super::with_session(&key, |sess, fresh| {
                if fresh {
                    sess.source = "opencode".into();
                    sess.client = "terminal".into();
                    sess.project = super::file_name(cwd.trim_end_matches(['/', '\\']));
                    sess.cwd = cwd.clone();
                    sess.status = "idle".into();
                    sess.action = "Готов к работе".into();
                    sess.started_ms = if created > 0 { created } else { now };
                    sess.updated_ms = mt;
                }
                // opencode names new sessions "New session - <date>" until it makes a title
                let t = if title.starts_with("New session") { "" } else { title.as_str() };
                if sess.title != t {
                    sess.title = super::short(t, 60);
                    return true;
                }
                fresh
            });
        }
    }
    changed
}

/// Status of one session from its newest message and that message's parts.
pub fn read(root: &Path, f: &mut OcFile, now: i64) -> bool {
    let msgs = sorted_files(&root.join("message").join(&f.id));
    let Some(last) = msgs.last() else { return false };
    let Some(m) = read_json(last) else { return false };
    let msg_id = s(&m, &["id"]).to_string();
    let parts = sorted_files(&root.join("part").join(&msg_id));
    let mut mtime = super::mtime_ms(last);
    for p in parts.iter().rev().take(2) {
        mtime = mtime.max(super::mtime_ms(p));
    }
    f.mtime = f.mtime.max(mtime);
    let quiet = now - mtime;

    let role = s(&m, &["role"]);
    let completed = n(&m, &["time", "completed"]) > 0.0;
    let finish = s(&m, &["finish"]);
    let mut status = "thinking";
    let mut action = "Думает…".to_string();
    let mut new_tool = false;
    let mut done = false;
    if role == "assistant" && completed && finish != "tool-calls" && finish != "unknown" {
        status = "done";
        action = "Готово — ждёт задачу".into();
        done = f.finished_msg != msg_id;
        f.finished_msg = msg_id.clone();
    } else if role == "assistant" {
        if let Some(tool) = parts.iter().rev().filter_map(|p| read_json(p)).find(|p| s(p, &["type"]) == "tool") {
            let st = s(&tool, &["state", "status"]);
            let pid = s(&tool, &["id"]).to_string();
            if st == "running" || st == "pending" {
                status = "working";
                action = tool_action(&tool);
                if quiet > 15_000 && matches!(s(&tool, &["tool"]), "bash" | "edit" | "write" | "patch" | "webfetch") {
                    status = "waiting";
                    action = format!("Ждёт подтверждения в терминале? {action}");
                }
            }
            if st == "completed" && f.last_tool != pid {
                f.last_tool = pid;
                new_tool = true;
            }
        }
    }
    if status == "thinking" && quiet > 120_000 {
        status = "idle";
        action = "Ждёт".into();
    }
    let used = n(&m, &["tokens", "input"]) + n(&m, &["tokens", "cache", "read"]) + n(&m, &["tokens", "output"]);
    let done_recent = done && quiet < 5 * 60 * 1000;
    let changed = super::with_session(&f.key, |sess, fresh| {
        if fresh {
            return false; // not discovered yet
        }
        let before = (sess.status.clone(), sess.action.clone(), sess.actions);
        sess.status = status.into();
        sess.action = action.clone();
        if new_tool {
            sess.actions += 1;
        }
        if used > 0.0 {
            sess.context = (used / 200_000.0).min(1.0);
        }
        sess.updated_ms = sess.updated_ms.max(mtime);
        before != (sess.status.clone(), sess.action.clone(), sess.actions)
    });
    if new_tool {
        super::bump_activity();
    }
    if done_recent {
        super::bump_done();
    }
    changed
}

fn tool_action(p: &Value) -> String {
    let input = p.get("state").and_then(|s| s.get("input")).cloned().unwrap_or(Value::Null);
    let file = || super::file_name(s(&input, &["filePath"]));
    match s(p, &["tool"]) {
        "edit" | "write" | "patch" | "multiedit" => {
            let f = file();
            if f.is_empty() { "Правит файлы".into() } else { format!("Правит {f}") }
        }
        "read" => {
            let f = file();
            if f.is_empty() { "Читает код".into() } else { format!("Читает {f}") }
        }
        "grep" | "glob" | "list" | "codesearch" => "Ищет по коду".into(),
        "webfetch" | "websearch" => "Смотрит в интернете".into(),
        "task" => "Отдал задачу помощнику".into(),
        "todowrite" | "todoread" => "Планирует".into(),
        "bash" => {
            let cmd = s(&input, &["command"]);
            if cmd.contains("test") {
                "Запускает тесты".into()
            } else {
                format!("Запускает: {}", super::short(cmd, 40))
            }
        }
        other => format!("Работает: {other}"),
    }
}
