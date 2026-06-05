//! # mlx-agent-tools
//!
//! Tool trait, core types, and built-in tool implementations for the
//! MLX-Pilot agent. This crate defines the interface that all tools
//! (file I/O, exec, web, etc.) must implement.

pub mod checkpoints;
pub mod sandbox;
pub mod scheduler;
pub mod tool;
pub mod tools;
pub mod types;

// Re-exports for convenience.
pub use checkpoints::{
    list_file_checkpoints, record_file_checkpoint, restore_file_checkpoint, FileCheckpointRecord,
    FileCheckpointRestoreResult, FileCheckpointSummary,
};
pub use scheduler::schedule_exec_task;
pub use tool::Tool;
pub use tools::{
    EditFileTool, ExecTool, GlobTool, GrepTool, ListDirTool, ReadFileTool, WriteFileTool,
};
pub use types::{
    ExecutionDomain, ExecutionMode, ExecutionPriority, ParamSchema, ToolContext, ToolDefinition,
    ToolError, ToolResult,
};
