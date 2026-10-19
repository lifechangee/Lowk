//! OpenAI-compatible streaming client, ChatLedger, PromptBuilder. M0 stub.
use lowk_types::Result;

/// Append-only ledger of serialized messages (prompt-cache prefix stays
/// byte-identical). Messages are stored as final bytes, never re-rendered.
#[derive(Default, Debug, Clone)]
pub struct ChatLedger {
    messages: Vec<String>,
}
impl ChatLedger {
    pub fn append(&mut self, serialized: String) {
        self.messages.push(serialized);
    }
    pub fn messages(&self) -> &[String] {
        &self.messages
    }
}

pub trait ChatClient: Send + Sync {
    /// Streams a completion; `on_delta` receives text deltas. Cancel by drop.
    fn stream(&self, ledger: &ChatLedger, on_delta: &mut dyn FnMut(&str)) -> Result<String>;
}

/// Answers with a fixed string, streamed in small pieces.
pub struct FakeLlm(pub String);
impl ChatClient for FakeLlm {
    fn stream(&self, _ledger: &ChatLedger, on_delta: &mut dyn FnMut(&str)) -> Result<String> {
        for w in self.0.split_inclusive(' ') {
            on_delta(w);
        }
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_prefix_is_stable() {
        let mut l = ChatLedger::default();
        l.append("a".into());
        let before = l.messages().to_vec();
        l.append("b".into());
        assert_eq!(&l.messages()[..1], &before[..]);
    }
}
