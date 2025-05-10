// src/event.rs
use serde::{Deserialize, Serialize};

/// Represents a file system event that can be broadcast or received.
///
/// # Fields
/// - `path`: The path of the file or directory affected by the event.
/// - `op`: The operation performed. One of: "CREATE", "WRITE", "REMOVE", "RENAME".
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Event {
    /// The path of the file or directory affected by the event.
    pub path: String,
    /// The operation performed. One of: "CREATE", "WRITE", "REMOVE", "RENAME".
    pub op: String, // "CREATE", "WRITE", "REMOVE", "RENAME"
}
