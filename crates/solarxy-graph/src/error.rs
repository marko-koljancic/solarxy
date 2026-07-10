//! Graph error type. Library convention: `thiserror` here, `anyhow` only
//! in binary shells.
//!
//! These are **command failures** (the caller did something structurally
//! illegal). Cook failures are data, not errors: they surface as
//! `CookStatus::Error` badge events per the boundary design, so they are
//! deliberately not represented here.

use crate::document::{EdgeId, NodeId};

/// Errors produced by document mutation and engine commands.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("node {0:?} does not exist in this graph")]
    UnknownNode(NodeId),

    #[error("edge {0:?} does not exist in this graph")]
    UnknownEdge(EdgeId),

    #[error("node {node:?} has no port '{port}'")]
    UnknownPort { node: NodeId, port: String },

    #[error("connection would create a cycle")]
    CycleDetected,

    #[error("input '{port}' on {node:?} is already connected (single-arity port)")]
    PortOccupied { node: NodeId, port: String },

    #[error("connection type mismatch: {from} does not coerce to {to}")]
    TypeMismatch { from: String, to: String },

    #[error("unknown node type '{0}'")]
    UnknownNodeType(String),

    #[error("node type '{type_id}' is not allowed in the {context} context")]
    ContextIllegal { type_id: String, context: String },

    #[error("'{port}' on {node:?} is not a variadic port")]
    NotVariadic { node: NodeId, port: String },

    #[error("reorder list for '{port}' on {node:?} is not a permutation of its current edges")]
    InvalidReorder { node: NodeId, port: String },

    #[error("no graph exists for the requested context")]
    UnknownContext,

    #[error("param '{key}' does not exist on this node type")]
    UnknownParam { key: String },

    #[error("param '{key}' rejected: {reason}")]
    InvalidParamValue { key: String, reason: String },

    #[error("annotation {0:?} does not exist")]
    UnknownAnnotation(crate::review::AnnotationId),

    #[error("invalid reply: {0}")]
    InvalidReply(&'static str),

    #[error("registry is invalid: {0}")]
    InvalidRegistry(String),
}
