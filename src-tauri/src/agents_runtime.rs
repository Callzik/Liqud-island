use std::collections::HashSet;

use serde_json::Value;

/// A Claude-compatible hook can also be installed in Codex. Validate its origin.
pub(super) fn claude_hook_source(transcript: &str, parents: &[String], first_kind: Option<&str>) -> bool {
    if first_kind == Some("session_meta") {
        return false;
    }
    let path = transcript.replace('\\', "/").to_ascii_lowercase();
    if path.contains("/.codex/") || path.rsplit('/').next().unwrap_or("").starts_with("rollout-") {
        return false;
    }
    if path.contains("/.claude/") {
        return true;
    }
    if matches!(first_kind, Some("user" | "assistant" | "queue-operation" | "file-history-snapshot" | "attachment")) {
        return true;
    }
    // Nearest agent wins when one client launches a different agent.
    for parent in parents {
        let normalized = parent.replace('\\', "/").to_ascii_lowercase();
        match normalized.rsplit('/').next().unwrap_or("") {
            "claude.exe" => return true,
            "codex.exe" | "chatgpt.exe" => return false,
            _ => {}
        }
    }
    true
}

/// The app-server can record "vscode" for both Desktop and the CLI.
pub(super) fn codex_client(meta: &Value) -> Option<&'static str> {
    if meta.pointer("/source/subagent/other").and_then(Value::as_str) == Some("guardian") {
        return None;
    }
    let origin = meta.get("originator").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let source = meta.get("source").and_then(Value::as_str).unwrap_or("");
    Some(if origin.contains("desktop") {
        "desktop"
    } else if origin.contains("vscode") {
        "vscode"
    } else if matches!(origin.as_str(), "codex-tui" | "codex_cli_rs" | "codex-cli") {
        "terminal"
    } else if source == "vscode" {
        "vscode"
    } else {
        "terminal"
    })
}

/// An ancestor from another app can launch this agent, but it is not its window.
pub(super) fn host_exes(client: &str, source: &str) -> &'static [&'static str] {
    match (client, source) {
        ("desktop", "codex") => &["codex.exe"],
        ("desktop", "claude") => &["claude.exe"],
        ("desktop", _) => &[],
        ("vscode", _) => &["code.exe", "code - insiders.exe", "vscodium.exe"],
        ("cursor", _) => &["cursor.exe"],
        ("windsurf", _) => &["windsurf.exe"],
        ("jetbrains", _) => &[
            "idea64.exe", "pycharm64.exe", "webstorm64.exe", "rider64.exe", "goland64.exe",
            "clion64.exe", "phpstorm64.exe", "rustrover64.exe",
        ],
        _ => &[
            "windowsterminal.exe", "conhost.exe", "wezterm-gui.exe", "alacritty.exe",
            "mintty.exe", "conemu64.exe", "conemu.exe",
        ],
    }
}

#[derive(Default)]
pub(super) struct CodexCalls {
    ids: HashSet<String>,
    unnamed: usize,
}

impl CodexCalls {
    pub(super) fn start(&mut self, id: &str) {
        if id.is_empty() {
            self.unnamed += 1;
        } else {
            self.ids.insert(id.to_owned());
        }
    }

    pub(super) fn finish(&mut self, id: &str) -> bool {
        if id.is_empty() && self.unnamed > 0 {
            self.unnamed -= 1;
            true
        } else {
            self.ids.remove(id)
        }
    }

    pub(super) fn running(&self) -> bool {
        self.unnamed != 0 || !self.ids.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.ids.clear();
        self.unnamed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn copied_claude_hook_rejects_codex_rollouts_and_processes() {
        assert!(!claude_hook_source(r"C:\Users\me\.codex\sessions\rollout-1.jsonl", &[], None));
        assert!(!claude_hook_source("/custom/sessions/rollout-2.jsonl", &[], Some("session_meta")));
        assert!(!claude_hook_source("", &[r"C:\apps\codex.exe".into(), r"C:\apps\Claude.exe".into()], None));
        assert!(!claude_hook_source("", &[r"C:\apps\ChatGPT.exe".into()], None));
        assert!(!claude_hook_source(r"C:\custom\session.jsonl", &[], Some("session_meta")));
    }

    #[test]
    fn genuine_claude_hooks_work_even_when_launched_by_codex() {
        assert!(claude_hook_source(
            r"C:\Users\me\.claude\projects\x\session.jsonl",
            &[r"C:\apps\codex.exe".into()],
            Some("user")
        ));
        assert!(claude_hook_source("", &[r"C:\apps\claude.exe".into(), r"C:\apps\codex.exe".into()], None));
        assert!(claude_hook_source("/custom/claude/session.jsonl", &[], Some("assistant")));
        assert!(claude_hook_source(
            "/custom/claude/session.jsonl",
            &[r"C:\apps\node.exe".into(), r"C:\apps\codex.exe".into()],
            Some("user")
        ));
        assert!(claude_hook_source("", &[], None));
    }

    #[test]
    fn desktop_and_cli_identity_beat_the_app_server_default() {
        for (originator, expected) in [
            ("Codex Desktop", "desktop"),
            ("codex_work_desktop", "desktop"),
            ("codex-tui", "terminal"),
            ("codex_cli_rs", "terminal"),
            ("codex_vscode", "vscode"),
        ] {
            assert_eq!(codex_client(&json!({ "originator": originator, "source": "vscode" })), Some(expected));
        }
    }

    #[test]
    fn only_internal_guardian_sessions_are_hidden() {
        assert_eq!(codex_client(&json!({"source":{"subagent":{"other":"guardian"}}})), None);
        assert_eq!(
            codex_client(&json!({"originator":"Codex Desktop","source":{"subagent":{"thread_spawn":{}}}})),
            Some("desktop")
        );
    }

    #[test]
    fn desktop_hosts_do_not_cross_agent_providers() {
        assert_eq!(host_exes("desktop", "codex"), ["codex.exe"]);
        assert_eq!(host_exes("desktop", "claude"), ["claude.exe"]);
        assert!(host_exes("desktop", "opencode").is_empty());
        assert!(!host_exes("terminal", "claude").contains(&"codex.exe"));
        assert!(!host_exes("terminal", "codex").contains(&"claude.exe"));
    }

    #[test]
    fn tool_outputs_only_end_their_own_calls() {
        let mut calls = CodexCalls::default();
        calls.start("slow");
        calls.start("fast");
        assert!(calls.finish("fast"));
        assert!(!calls.finish("fast"));
        assert!(calls.running());
        calls.finish("slow");
        assert!(!calls.running());
        calls.start("");
        assert!(!calls.finish("fast"));
        assert!(calls.running());
        assert!(calls.finish(""));
        assert!(!calls.running());
        calls.start("aborted");
        calls.clear();
        assert!(!calls.running());
    }
}
