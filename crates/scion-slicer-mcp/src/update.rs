//! Update notice.
//!
//! The server never talks to the network. The npm launcher (`npm/mcp`) checks the npm
//! registry at most once a day and writes what it learned to a small JSON file, whose
//! path it passes in `SCION_SLICER_UPDATE_FILE`. The server reads that file when it needs
//! to know whether a newer version exists. Without the launcher (a source build) there is
//! no file and no notice.
//!
//! The agent plugin always runs the latest server (`npx @scion-slicer/mcp@latest`), so
//! the server itself is rarely out of date; the plugin's skills are. The plugin passes its
//! own version in `SCION_SLICER_PLUGIN_VERSION`, and when it is set, updates are measured
//! against the plugin rather than the server.

use serde::Deserialize;

/// The running version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the launcher says the update file is.
pub const UPDATE_FILE_ENV: &str = "SCION_SLICER_UPDATE_FILE";

/// The plugin's version, set in the plugin's MCP files.
pub const PLUGIN_VERSION_ENV: &str = "SCION_SLICER_PLUGIN_VERSION";

/// Set to turn the check off; the launcher honours it, and so does the server.
pub const OPT_OUT_ENV: &str = "SCION_SLICER_NO_UPDATE_CHECK";

pub const CHANGELOG_URL: &str =
    "https://github.com/scion-slicer/scion-slicer/blob/main/CHANGELOG.md";

#[derive(Debug, Deserialize)]
struct UpdateFile {
    latest: Option<String>,
}

/// Whether update checks are on for this process.
pub fn check_enabled() -> bool {
    std::env::var_os(OPT_OUT_ENV).is_none_or(|v| v.is_empty() || v == "0")
}

/// The latest version the launcher has seen on npm, if any.
pub fn latest_known() -> Option<String> {
    if !check_enabled() {
        return None;
    }
    let path = std::env::var_os(UPDATE_FILE_ENV)?;
    let text = std::fs::read_to_string(path).ok()?;
    let file: UpdateFile = serde_json::from_str(&text).ok()?;
    file.latest.filter(|v| parse(v).is_some())
}

/// The plugin's version, when the server runs from the agent plugin.
pub fn plugin_version() -> Option<String> {
    std::env::var(PLUGIN_VERSION_ENV)
        .ok()
        .filter(|v| parse(v).is_some())
}

/// The latest known version, when it is newer than what is installed: the plugin when
/// running from it, otherwise this server.
pub fn newer_version() -> Option<String> {
    let installed = plugin_version().unwrap_or_else(|| VERSION.to_string());
    latest_known().filter(|latest| is_newer(latest, &installed))
}

/// `X.Y.Z` with plain numbers. Pre-releases and anything else are not offered as updates.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(c), Some(r)) => c > r,
        _ => false,
    }
}

/// The client in use, from the name it sent when it connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeCode,
    CopilotCli,
    VsCode,
    Codex,
    Cursor,
    Other,
}

impl Client {
    pub fn from_name(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        if name.contains("claude") {
            Client::ClaudeCode
        } else if name.contains("codex") {
            Client::Codex
        } else if name.contains("cursor") {
            Client::Cursor
        } else if name.contains("copilot") {
            Client::CopilotCli
        } else if name.contains("visual studio code") || name.contains("vscode") {
            Client::VsCode
        } else {
            Client::Other
        }
    }
}

/// How to update in the given client.
pub fn how_to_update(client: Client) -> &'static str {
    match client {
        Client::ClaudeCode => {
            "run `claude plugin marketplace update scion-slicer` and \
             `claude plugin update scion-slicer@scion-slicer`, then restart Claude Code"
        }
        Client::CopilotCli => "run `copilot plugin update scion-slicer@scion-slicer`",
        Client::VsCode => "update the scion-slicer plugin in VS Code's agent plugins view",
        Client::Codex => {
            "run `codex plugin marketplace upgrade`, then `codex plugin add scion-slicer@scion-slicer`"
        }
        Client::Cursor => "refresh the scion-slicer plugin in Cursor's plugin settings",
        Client::Other => {
            "update the scion-slicer plugin in your agent, or run the server with \
             `npx -y @scion-slicer/mcp@latest`"
        }
    }
}

/// The one-line notice.
pub fn notice(latest: &str, client: Client) -> String {
    notice_for(latest, plugin_version().as_deref(), client)
}

fn notice_for(latest: &str, plugin: Option<&str>, client: Client) -> String {
    let what = match plugin {
        Some(installed) => {
            format!("The Scion Slicer plugin {latest} is available (you have {installed})")
        }
        None => format!("Scion Slicer {latest} is available (this is {VERSION})"),
    };
    format!(
        "{what}. To update, {}. Changes: {CHANGELOG_URL}",
        how_to_update(client)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn ignores_versions_it_cannot_read() {
        assert!(!is_newer("0.3.0-beta.1", "0.1.0"));
        assert!(!is_newer("latest", "0.1.0"));
        assert!(!is_newer("1.2", "0.1.0"));
        assert!(!is_newer("1.2.3.4", "0.1.0"));
    }

    #[test]
    fn notice_names_the_plugin_when_running_from_it() {
        let from_plugin = notice_for("0.2.0", Some("0.1.1"), Client::CopilotCli);
        assert!(
            from_plugin.starts_with("The Scion Slicer plugin 0.2.0 is available (you have 0.1.1)")
        );
        assert!(from_plugin.contains("copilot plugin update"));
        let plain = notice_for("0.2.0", None, Client::Other);
        assert!(plain.starts_with(&format!(
            "Scion Slicer 0.2.0 is available (this is {VERSION})"
        )));
    }

    #[test]
    fn recognises_clients() {
        assert_eq!(Client::from_name("claude-code"), Client::ClaudeCode);
        assert_eq!(Client::from_name("codex-mcp-client"), Client::Codex);
        assert_eq!(Client::from_name("cursor-vscode"), Client::Cursor);
        assert_eq!(Client::from_name("github-copilot-cli"), Client::CopilotCli);
        assert_eq!(Client::from_name("Visual Studio Code"), Client::VsCode);
        assert_eq!(Client::from_name("mcp-inspector"), Client::Other);
    }
}
