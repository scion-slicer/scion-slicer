//! Scion Slicer MCP server (stdio).
//!
//! Tools:
//! - `check_print`: run the print checks on a binary STL and return the findings
//!   (resin traps, suction cups, peel alerts, ledges) as JSON.
//! - `list_printers`, `list_resins`: the printers and resin profiles to slice with,
//!   ELEGOO SatelLite's when it's installed.
//! - `slice`: slice a binary STL into a printer file (`.goo`).
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
use scion_slicer_core::profiles::Catalog;
use scion_slicer_core::slice::{
    slice_stl, BottomCompensation, Placement, SliceOptions, SliceRequest,
};
use scion_slicer_core::{check_stl, DEFAULT_LAYER_HEIGHT_MM, DEFAULT_PX_MM};
use update::Client;

const INSTRUCTIONS: &str = "Scion Slicer: resin 3D printing tools built on the DragonFruit \
    slicing engine. Use check_print on an STL in print orientation to find resin traps, suction \
    cups, peel problems and ledges before slicing. Use list_printers and list_resins to see the \
    printers and resin profiles, and slice to turn an STL into a file the printer reads (.goo for \
    ELEGOO printers). If a tool result ends with a note that a newer Scion Slicer version is \
    available, pass that note on to the user once.";

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

/// Arguments of `list_resins`.
#[derive(Debug, Deserialize, JsonSchema)]
struct ListResinsArgs {
    /// Printer name, or part of it. Default: the first printer.
    printer: Option<String>,
}

/// Arguments of `slice`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SliceArgs {
    /// Absolute path to a binary STL, already in print orientation (lowest point is what
    /// touches the build plate) and with any supports it needs.
    stl_path: String,
    /// Where to write the printer file. Default: next to the STL, with a .goo extension.
    output_path: Option<String>,
    /// Printer name, or part of it (see list_printers). Default: the first printer.
    printer: Option<String>,
    /// Resin profile name, or part of it (see list_resins). Default: the profile ELEGOO
    /// SatelLite last used.
    resin: Option<String>,
    /// Turn the part about the vertical axis by this many degrees first.
    rotate_deg: Option<f64>,
    /// Where the corner of the part's bounding box goes, in mm from the corner of the
    /// screen: [x, y]. Default: centred.
    position_mm: Option<[f64; 2]>,
    /// Elephant's-foot compensation for outer outlines on the bottom layers, in mm
    /// (negative shrinks). Default 0.
    outline_compensation_mm: Option<f32>,
    /// Compensation for holes on the bottom layers, in mm (negative opens them). Default 0.
    hole_compensation_mm: Option<f32>,
    /// Fade the compensation out over the transition layers. Default true.
    fade_compensation: Option<bool>,
    /// Replace the output file if it exists. Default false.
    overwrite: Option<bool>,
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

    /// The printers to slice for.
    #[tool(
        name = "list_printers",
        description = "List the resin printers Scion Slicer can slice for: ELEGOO SatelLite's printers \
                       when it's installed on this computer, otherwise the built-in ones. Shows each \
                       printer's resolution, screen size and resin profiles."
    )]
    async fn list_printers(&self, peer: Peer<RoleServer>) -> Result<CallToolResult, String> {
        let catalog = Catalog::load();
        let printers: Vec<_> = catalog
            .printers
            .iter()
            .map(|p| {
                let m = &p.machine;
                serde_json::json!({
                    "name": m.name,
                    "resolution_px": [m.resolution_x, m.resolution_y],
                    "screen_mm": [m.display_width_mm, m.display_height_mm],
                    "height_mm": m.machine_z_mm,
                    "file_format": "goo",
                    "resin_profiles": p.resins.len(),
                    "active_resin": p.active_resin,
                })
            })
            .collect();
        let json = serde_json::json!({ "source": catalog.source, "problem": catalog.problem, "printers": printers });
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        Ok(self.with_update_notice(
            CallToolResult::success(vec![ContentBlock::text(text)]),
            &peer,
        ))
    }

    /// The resin profiles of a printer.
    #[tool(
        name = "list_resins",
        description = "List the resin profiles for a printer, with their layer height, exposures, \
                       bottom and transition layer counts and waits."
    )]
    async fn list_resins(
        &self,
        Parameters(args): Parameters<ListResinsArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, String> {
        let catalog = Catalog::load();
        let printer = catalog.printer(args.printer.as_deref())?;
        let resins: Vec<_> = printer
            .resins
            .iter()
            .map(|r| {
                let waits = |w: &scion_slicer_goo::Waits| {
                    serde_json::json!({
                        "before_cure_s": w.before_cure_s,
                        "before_release_s": w.before_release_s,
                        "after_release_s": w.after_release_s,
                    })
                };
                serde_json::json!({
                    "name": r.profile_name,
                    "active": printer.active_resin.as_deref() == Some(r.profile_name.as_str()),
                    "layer_height_mm": r.layer_height_mm,
                    "bottom_layers": r.bottom_layers,
                    "transition_layers": r.transition_layers,
                    "bottom_exposure_s": r.bottom.exposure_s,
                    "exposure_s": r.normal.exposure_s,
                    "bottom_waits": waits(&r.bottom.waits),
                    "waits": waits(&r.normal.waits),
                    "density_g_ml": r.density_g_ml,
                })
            })
            .collect();
        let json = serde_json::json!({ "printer": printer.machine.name, "source": catalog.source, "resins": resins });
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        Ok(self.with_update_notice(
            CallToolResult::success(vec![ContentBlock::text(text)]),
            &peer,
        ))
    }

    /// Slice a mesh into a printer file.
    #[tool(
        name = "slice",
        description = "Slice a binary STL into a file the printer reads (.goo for ELEGOO printers), with \
                       a printer and resin profile from list_printers / list_resins. The part goes on \
                       the plate as it is oriented (lowest point down), centred unless a position is \
                       given, optionally turned about the vertical axis. Bottom-layer compensation \
                       for elephant's foot can shrink outlines and open holes. Returns the file's path, \
                       layer count, print time, resin volume and weight. Run check_print first."
    )]
    async fn slice(
        &self,
        Parameters(args): Parameters<SliceArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, String> {
        let compensation = match (
            args.outline_compensation_mm.unwrap_or(0.0),
            args.hole_compensation_mm.unwrap_or(0.0),
        ) {
            (0.0, 0.0) => None,
            (outline_mm, hole_mm) => Some(BottomCompensation {
                outline_mm,
                hole_mm,
                fade: args.fade_compensation.unwrap_or(true),
            }),
        };
        let request = SliceRequest {
            stl_path: PathBuf::from(&args.stl_path),
            output_path: args.output_path.map(PathBuf::from),
            printer: args.printer,
            resin: args.resin,
            options: SliceOptions {
                placement: match args.position_mm {
                    Some([x_mm, y_mm]) => Placement::Corner { x_mm, y_mm },
                    None => Placement::Centered,
                },
                rotate_z_deg: args.rotate_deg.unwrap_or(0.0),
                bottom_compensation: compensation,
            },
            overwrite: args.overwrite.unwrap_or(false),
        };
        // Slicing is CPU-bound; keep it off the async runtime's threads.
        let report = tokio::task::spawn_blocking(move || slice_stl(&request, &Catalog::load()))
            .await
            .map_err(|e| format!("slicing stopped unexpectedly: {e}"))??;
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
