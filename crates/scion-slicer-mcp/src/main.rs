//! Scion Slicer MCP server (stdio).
//!
//! Tools:
//! - `check_print`: run the print checks on a binary STL and return the findings
//!   (resin traps, suction cups, peel alerts, ledges) as JSON.
//! - `about`: the running version, whether a newer one exists, and how to update.
//!
//! When the npm launcher has seen a newer version on npm (see `update.rs`), the server
//! says so once per session: on stderr shortly after the client connects, and as an extra
//! line on the first tool result after it learns of it. (MCP's own logging messages would
//! be the other place, but SEP-2577 deprecates them.)

mod update;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    service::NotificationContext,
    tool, tool_handler, tool_router, Peer, RoleServer, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;

use scion_slicer_core::checks::ChecksConfig;
use scion_slicer_core::{check_stl, DEFAULT_LAYER_HEIGHT_MM, DEFAULT_PX_MM};
use update::Client;

const INSTRUCTIONS: &str = "Scion Slicer: resin 3D printing tools built on the DragonFruit \
    slicing engine. Use check_print on an STL in print orientation to find resin traps, suction \
    cups, peel problems and ledges before slicing. If a tool result ends with a note that a newer \
    Scion Slicer version is available, pass that note on to the user once.";

/// How long after the client connects to look for the launcher's update result. The
/// launcher's registry request times out after 2 seconds.
const STDERR_NOTICE_DELAY: Duration = Duration::from_secs(3);

/// Arguments of `check_print`.
#[derive(Debug, Deserialize, JsonSchema)]
struct CheckPrintArgs {
    /// Absolute path to a binary STL, already in print orientation (lowest point is
    /// what touches the build plate).
    stl_path: String,
    /// Pixel size for the analysis, in mm. Default 0.05.
    px_mm: Option<f64>,
    /// Layer height, in mm. Default 0.05.
    layer_height_mm: Option<f64>,
}

#[derive(Debug, Clone)]
struct ScionSlicer {
    tool_router: ToolRouter<Self>,
    /// Set once the update notice has been added to a tool result.
    tool_notice_given: Arc<AtomicBool>,
}

impl ScionSlicer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            tool_notice_given: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Adds the update notice to `result` the first time a newer version is known.
    fn with_update_notice(
        &self,
        mut result: CallToolResult,
        peer: &Peer<RoleServer>,
    ) -> CallToolResult {
        if self.tool_notice_given.load(Ordering::Relaxed) {
            return result;
        }
        if let Some(latest) = update::newer_version() {
            if !self.tool_notice_given.swap(true, Ordering::Relaxed) {
                result
                    .content
                    .push(ContentBlock::text(update::notice(&latest, client_of(peer))));
            }
        }
        result
    }
}

fn client_of(peer: &Peer<RoleServer>) -> Client {
    peer.peer_info()
        .map(|info| Client::from_name(&info.client_info.name))
        .unwrap_or(Client::Other)
}

#[tool_router(router = tool_router)]
impl ScionSlicer {
    /// Run the print checks on a mesh.
    #[tool(
        name = "check_print",
        description = "Check a resin print before slicing. Takes a binary STL in print orientation and \
                       reports resin traps (sealed pockets), suction cups (pockets open toward the vat \
                       film), peel alerts (sudden jumps in layer area, and the largest layer) and ledges \
                       (overhangs, with their depth in mm). Each finding has its layer range, location \
                       in mm, size, cause and a suggested action."
    )]
    async fn check_print(
        &self,
        Parameters(args): Parameters<CheckPrintArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, String> {
        let path = PathBuf::from(&args.stl_path);
        let px_mm = args.px_mm.unwrap_or(DEFAULT_PX_MM);
        let layer_height_mm = args.layer_height_mm.unwrap_or(DEFAULT_LAYER_HEIGHT_MM);
        // The checks are CPU-bound; keep them off the async runtime's threads.
        let report = tokio::task::spawn_blocking(move || {
            check_stl(&path, px_mm, layer_height_mm, &ChecksConfig::default())
        })
        .await
        .map_err(|e| format!("the check stopped unexpectedly: {e}"))??;
        let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        Ok(self.with_update_notice(
            CallToolResult::success(vec![ContentBlock::text(json)]),
            &peer,
        ))
    }

    /// Version and update information.
    #[tool(
        name = "about",
        description = "Show the running Scion Slicer version, the latest version known on npm, \
                       whether an update is available, and how to update in this agent."
    )]
    async fn about(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let latest = update::latest_known();
        let newer = update::newer_version();
        let client = client_of(&peer);
        let info = serde_json::json!({
            "name": "scion-slicer",
            "version": update::VERSION,
            "plugin_version": update::plugin_version(),
            "latest_known": latest,
            "update_available": newer.is_some(),
            "how_to_update": newer.as_ref().map(|_| update::how_to_update(client)),
            "update_check": if update::check_enabled() {
                "on: the npm launcher checks at most once a day"
            } else {
                "off (SCION_SLICER_NO_UPDATE_CHECK is set)"
            },
            "changelog": update::CHANGELOG_URL,
            "homepage": "https://github.com/scion-slicer/scion-slicer",
        });
        if newer.is_some() {
            // `about` reports the update itself, so no extra note is needed later.
            self.tool_notice_given.store(true, Ordering::Relaxed);
        }
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ScionSlicer {
    fn get_info(&self) -> ServerConfig {
        let mut server = Implementation::new("scion-slicer", update::VERSION);
        server.title = Some("Scion Slicer".into());
        server.website_url = Some("https://github.com/scion-slicer/scion-slicer".into());
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(server)
            .with_instructions(INSTRUCTIONS)
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if !update::check_enabled() {
            return;
        }
        let peer = context.peer;
        tokio::spawn(async move {
            tokio::time::sleep(STDERR_NOTICE_DELAY).await;
            if let Some(latest) = update::newer_version() {
                eprintln!(
                    "scion-slicer: {}",
                    update::notice(&latest, client_of(&peer))
                );
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--version") {
        println!("scion-slicer-mcp {}", update::VERSION);
        return Ok(());
    }
    let service = ScionSlicer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
