//! Append-only chat ledger: every message is stored as its final serialized JSON bytes and is
//! never re-rendered, so the prompt-cache prefix of request n+1 is byte-identical to request n.
use serde_json::json;

/// One compaction of the ledger, kept for debugging (not sent to the model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionEvent {
    /// Messages replaced by the one summary / notice message.
    pub replaced: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
    /// `true` = LLM summary; `false` = plain truncation (fallback or no summary model).
    pub summarized: bool,
}

#[derive(Default, Debug, Clone)]
pub struct ChatLedger {
    messages: Vec<String>,
    compactions: Vec<CompactionEvent>,
}

impl ChatLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an already serialized message object (`{"role":..,"content":..}`).
    pub fn append(&mut self, serialized: String) {
        self.messages.push(serialized);
    }

    pub fn append_system(&mut self, text: &str) {
        self.append(json!({ "role": "system", "content": text }).to_string());
    }

    pub fn append_user(&mut self, text: &str) {
        self.append(json!({ "role": "user", "content": text }).to_string());
    }

    /// The exact streamed text of an answer (history stays consistent after a cancel too).
    pub fn append_assistant(&mut self, text: &str) {
        self.append(json!({ "role": "assistant", "content": text }).to_string());
    }

    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Compactions applied to this conversation so far (oldest first).
    pub fn compactions(&self) -> &[CompactionEvent] {
        &self.compactions
    }

    pub(crate) fn from_parts(messages: Vec<String>, compactions: Vec<CompactionEvent>) -> Self {
        Self {
            messages,
            compactions,
        }
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// `[m0,m1,...]` built by concatenation: the stored bytes go on the wire unchanged.
    pub fn to_json_array(&self) -> String {
        format!("[{}]", self.messages.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_prefix_is_stable() {
        let mut l = ChatLedger::default();
        l.append_system("sys");
        l.append_user("привет \"мир\"");
        let before = l.to_json_array();
        l.append_assistant("ok");
        l.append_user("next");
        let after = l.to_json_array();
        assert!(after.starts_with(&before[..before.len() - 1]));
        assert_eq!(l.len(), 4);
        // valid JSON
        let v: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(v[1]["content"], "привет \"мир\"");
    }
}
