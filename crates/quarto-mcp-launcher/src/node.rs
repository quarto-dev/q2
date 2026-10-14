//! Node discovery for `q2 mcp`: a thin wrapper over the shared
//! `quarto-node-discovery` crate that fixes the error-message purpose.

pub use quarto_node_discovery::{Discovery, MIN_NODE_MAJOR, NodeError, NodeInfo, parse_version};

const PURPOSE: &str = "`q2 mcp`";

pub fn find_node(d: &Discovery) -> Result<NodeInfo, NodeError> {
    quarto_node_discovery::find_node(d, PURPOSE)
}
