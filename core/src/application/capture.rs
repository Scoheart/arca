//! Desktop-independent capture use cases.
pub use crate::capture::{AgentCaptureSupport, CaptureDelta, CaptureEnvironment, CaptureSession, CaptureSnapshot, CaptureTarget, CapturedFlow, FlowSummary, StartCapture};
pub fn environment() -> CaptureEnvironment { crate::capture::environment() }
pub use crate::capture::CaptureProxy;
pub fn capture_proxy(agent_id: &str) -> Result<CaptureProxy, String> { crate::capture::capture_proxy(agent_id) }
pub fn prepare_certificate() -> Result<std::path::PathBuf, String> { crate::capture::prepare_certificate() }
pub fn install_engine() -> Result<std::path::PathBuf, String> { crate::capture::install_engine() }
pub fn launch_agent(agent_id: &str, directory: Option<&str>) -> Result<CaptureTarget, String> { crate::capture::launch_agent(agent_id, directory) }
pub fn sessions() -> Result<Vec<CaptureSession>, String> { crate::capture::sessions() }
pub fn snapshot(id: &str) -> Result<CaptureSnapshot, String> { crate::capture::snapshot(id) }
pub fn detail(session: &str, flow: &str) -> Result<CapturedFlow, String> { crate::capture::detail(session, flow) }
// Capture only reads immutable configuration snapshots and writes its own
// private records. Its process lock and file lease do not block asset edits.
pub fn start(request: StartCapture) -> Result<CaptureSnapshot, String> { crate::capture::start(request) }
pub fn start_fresh(request: StartCapture, directory: Option<&str>) -> Result<CaptureSnapshot, String> { crate::capture::start_fresh(request, directory) }
pub fn stop(id: &str) -> Result<CaptureSnapshot, String> { crate::capture::stop(id) }
pub fn export(session: &str, flow: Option<&str>, path: &std::path::Path) -> Result<(), String> { crate::capture::export(session, flow, path) }

pub fn summaries(id: &str) -> Result<Vec<FlowSummary>, String> { crate::capture::summaries(id) }
pub fn delta(id: &str, revision: Option<&str>) -> Result<CaptureDelta, String> { crate::capture::delta(id, revision) }
