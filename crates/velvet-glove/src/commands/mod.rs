//! Interactive setup commands. Unlike hook commands, these write plain text
//! (or JSON) for a person or script at a terminal and never read hook
//! payloads.

pub mod check;
pub mod doctor;
pub mod init;
pub mod project;
pub mod tools;
