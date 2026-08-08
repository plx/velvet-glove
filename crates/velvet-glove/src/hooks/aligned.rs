//! Managed portable-handler exports for this harness set.
//!
//! Customize the handler files under `hooks/aligned/`; Copier can safely
//! refresh this selection-dependent module during recopy and update.
#[path = "aligned/post_tool.rs"]
mod post_tool_handler;
pub use post_tool_handler::handle as post_tool;
#[path = "aligned/turn_completion.rs"]
mod turn_completion_handler;
pub use turn_completion_handler::handle as turn_completion;
