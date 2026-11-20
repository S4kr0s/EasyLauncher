use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    /// A new phase started ("Downloading mods", "Installing Java" ...).
    Stage { text: String },
    /// Progress within the current phase.
    Progress { done: u64, total: u64 },
    /// A human-readable log line.
    Log { text: String },
    /// A line of game output.
    Game { line: String },
}

pub type Reporter = Arc<dyn Fn(Event) + Send + Sync>;

pub fn noop() -> Reporter {
    Arc::new(|_| {})
}

pub fn stage(r: &Reporter, text: impl Into<String>) {
    r(Event::Stage { text: text.into() })
}
pub fn log(r: &Reporter, text: impl Into<String>) {
    r(Event::Log { text: text.into() })
}
