//! Scion Slicer MCP server (stdio).
//!
//! Tools:
//! - `check_print`: run the print checks on a binary STL and return the findings
//!   (resin traps, suction cups, peel alerts, ledges) as JSON.

use std::path::PathBuf;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;

use scion_slicer_core::checks::ChecksConfig;
use scion_slicer_core::{check_stl, DEFAULT_LAYER_HEIGHT_MM, DEFAULT_PX_MM};

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
}

impl ScionSlicer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
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
    ) -> Result<String, String> {
        let path = PathBuf::from(&args.stl_path);
        let px_mm = args.px_mm.unwrap_or(DEFAULT_PX_MM);
        let layer_height_mm = args.layer_height_mm.unwrap_or(DEFAULT_LAYER_HEIGHT_MM);
        // The checks are CPU-bound; keep them off the async runtime's threads.
        let report = tokio::task::spawn_blocking(move || {
            check_stl(&path, px_mm, layer_height_mm, &ChecksConfig::default())
        })
        .await
        .map_err(|e| format!("the check stopped unexpectedly: {e}"))??;
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "scion-slicer",
    instructions = "Scion Slicer: resin 3D printing tools built on the DragonFruit slicing engine. \
                    Use check_print on an STL in print orientation to find resin traps, suction cups, \
                    peel problems and ledges before slicing."
)]
impl ServerHandler for ScionSlicer {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = ScionSlicer::new()
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
