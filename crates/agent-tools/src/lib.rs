//! # mlx-agent-tools
//!
//! Tool trait, core types, and built-in tool implementations for the
//! MLX-Pilot agent. This crate defines the interface that all tools
//! (file I/O, exec, web, etc.) must implement.

pub mod sandbox;
pub mod tool;
pub mod tools;
pub mod types;

// Re-exports for convenience.
pub use tool::Tool;
pub use tools::{EditFileTool, ExecTool, ListDirTool, ReadFileTool, WriteFileTool};
pub use types::{ExecutionMode, ParamSchema, ToolContext, ToolDefinition, ToolError, ToolResult};
