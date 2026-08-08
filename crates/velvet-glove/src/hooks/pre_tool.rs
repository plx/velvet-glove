//! Managed exports for the selected pre-tool policy archetype.
//!
//! Customize the active seam under `hooks/pre_tool/`; Copier can safely
//! refresh this selection-dependent module during recopy and update.
#[path = "pre_tool/decision.rs"]
mod decision;
pub use decision::Decision;
#[path = "pre_tool/policy.rs"]
mod policy;
pub use policy::evaluate;
