//! Compaction of the per-call conversation. The ledger is append-only (byte-stable prefix for
//! provider prompt caches); when it outgrows the budget, the oldest exchanges are replaced by ONE
//! "earlier in this call" message in a single big step, so the cache is rebuilt rarely.
//!
//! Layout kept: `[system, first user turn (folder context), <summary>, last N turns verbatim...]`.
//! The summary sits in an assistant message right after the first user turn, so roles still alternate.
//! Pure logic: the summary call goes through `ChatClient` (a stub in tests).
use crate::client::ChatClient;
use crate::ledger::{ChatLedger, CompactionEvent};
use lowk_types::{AppError, ErrorCode, Result};
use serde_json::Value;

/// Messages 0 (system) and 1 (first user turn with the folder context) are never compacted.
const HEAD: usize = 2;
/// Cap on the transcript handed to the summariser (chars); the oldest part is cut first.
const MAX_TRANSCRIPT_CHARS: usize = 120_000;

#[derive(Clone, Debug)]
pub struct CompactPolicy {
    /// Above this the ledger is compacted in the background after an answer (~60% of the window).
    pub soft_tokens: usize,
    /// Above this (with the new turn) compaction happens inline before the request (~85%).
    pub hard_tokens: usize,
    /// Newest user turns kept verbatim.
    pub keep_recent_turns: usize,
}

impl CompactPolicy {
    /// 60% soft / 85% hard of the model window.
    pub fn for_window(window_tokens: usize) -> Self {
        Self {
            soft_tokens: window_tokens * 60 / 100,
            hard_tokens: window_tokens * 85 / 100,
            keep_recent_turns: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactNeed {
    None,
    /// Compact after the answer, in the background.
    Soft,
    /// Compact before sending: the request would not fit.
    Hard,
}

/// Rough token estimate (no tokenizer): ~3.5 chars per token, images counted by a flat charge
/// instead of their base64 length.
pub fn estimate_tokens(ledger: &ChatLedger) -> usize {
    ledger.messages().iter().map(|m| estimate_message_tokens(m)).sum()
}

pub fn estimate_message_tokens(serialized: &str) -> usize {
    let mut chars = serialized.chars().count();
    // base64 screenshots: charge ~1000 tokens each instead of the data-URL length
    let mut rest = serialized;
    while let Some(i) = rest.find("data:image/") {
        let tail = &rest[i..];
        let end = tail.find('"').unwrap_or(tail.len());
        chars = chars.saturating_sub(end) + 3500;
        rest = &tail[end..];
    }
    chars * 2 / 7 + 4
}

impl CompactPolicy {
    pub fn need(&self, ledger: &ChatLedger, incoming_message: &str) -> CompactNeed {
        let now = estimate_tokens(ledger);
        if now + estimate_message_tokens(incoming_message) > self.hard_tokens {
            CompactNeed::Hard
        } else if now > self.soft_tokens {
            CompactNeed::Soft
        } else {
            CompactNeed::None
        }
    }
}

/// Which messages get replaced: `[HEAD, upto)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompactPlan {
    pub upto: usize,
}

impl CompactPlan {
    /// `None` when there is nothing older than the last `keep_recent_turns` user turns to fold.
    pub fn new(ledger: &ChatLedger, keep_recent_turns: usize) -> Option<Self> {
        let msgs = ledger.messages();
        let users: Vec<usize> = (HEAD..msgs.len()).filter(|&i| role(&msgs[i]) == "user").collect();
        let keep = keep_recent_turns.max(1);
        if users.len() < keep {
            return None;
        }
        let upto = users[users.len() - keep];
        // something beyond a lone previous summary must be folded
        (upto > HEAD).then_some(Self { upto })
    }

    /// Summary of `[HEAD, upto)` through `client`.
    pub fn summarize(&self, ledger: &ChatLedger, client: &dyn ChatClient, max_tokens: u32) -> Result<String> {
        let transcript = transcript_of(&ledger.messages()[HEAD..self.upto]);
        let mut l = ChatLedger::new();
        l.append_system(
            "You compress the earlier part of a live-call assistant conversation. Write a compact \
             factual summary: questions asked, answers given, decisions, names, numbers, terms and \
             open threads. Same language as the conversation. No preamble, at most 350 words.",
        );
        l.append_user(&format!(
            "<conversation>\n{transcript}\n</conversation>\nSummarise it."
        ));
        let opts = crate::config::ChatOptions {
            max_tokens: Some(max_tokens),
            temperature: Some(0.2),
            session_id: None,
        };
        let out = client.stream_ex(&l, &opts, &mut |_| {}, &|| true)?;
        let t = out.text.trim().to_string();
        if t.is_empty() {
            return Err(AppError::new(ErrorCode::LlmBadResponse).with_detail("empty compaction summary"));
        }
        Ok(t)
    }

    /// New ledger: head, one replacement message, the rest verbatim. `summary = None` = truncation.
    /// Messages appended to `ledger` after the plan was made are kept (they lie after `upto`).
    pub fn apply(&self, ledger: &ChatLedger, summary: Option<&str>) -> ChatLedger {
        let msgs = ledger.messages();
        let replaced = self.upto - HEAD;
        let text = match summary {
            Some(s) => format!("[Earlier in this call - summary of {replaced} earlier messages]\n{s}"),
            None => format!(
                "[Earlier in this call: {replaced} earlier messages were omitted to fit the context window]"
            ),
        };
        let mut out: Vec<String> = msgs[..HEAD].to_vec();
        out.push(serde_json::json!({ "role": "assistant", "content": text }).to_string());
        out.extend_from_slice(&msgs[self.upto..]);
        let mut ev = ledger.compactions().to_vec();
        let new = ChatLedger::from_parts(out, Vec::new());
        ev.push(CompactionEvent {
            replaced,
            tokens_before: estimate_tokens(ledger),
            tokens_after: estimate_tokens(&new),
            summarized: summary.is_some(),
        });
        ChatLedger::from_parts(new.messages().to_vec(), ev)
    }

    /// Still valid for `ledger` (it grew only by appending after the snapshot `base`).
    pub fn still_applies(&self, base: &ChatLedger, ledger: &ChatLedger) -> bool {
        let (a, b) = (base.messages(), ledger.messages());
        b.len() >= a.len()
            && a.len() > self.upto
            && a[..HEAD] == b[..HEAD]
            && a[self.upto - 1] == b[self.upto - 1]
    }
}

/// Summary (or truncation when it fails / is absent) in one call; never fails.
pub fn compact_ledger(
    ledger: &ChatLedger,
    policy: &CompactPolicy,
    client: Option<&dyn ChatClient>,
) -> Option<ChatLedger> {
    let plan = CompactPlan::new(ledger, policy.keep_recent_turns)?;
    let summary = client.and_then(|c| plan.summarize(ledger, c, 700).ok());
    Some(plan.apply(ledger, summary.as_deref()))
}

fn role(m: &str) -> String {
    serde_json::from_str::<Value>(m)
        .ok()
        .and_then(|v| v.get("role").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

/// Plain text of a message; images become a marker.
fn content_text(v: &Value) -> String {
    match v.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| match p.get("type").and_then(Value::as_str) {
                Some("text") => p.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                Some("image_url") => "[screenshot omitted]".to_string(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn transcript_of(msgs: &[String]) -> String {
    let mut out = String::new();
    for m in msgs {
        let Ok(v) = serde_json::from_str::<Value>(m) else {
            continue;
        };
        let who = match v.get("role").and_then(Value::as_str) {
            Some("user") => "USER",
            _ => "ASSISTANT",
        };
        out.push_str(&format!("{who}: {}\n\n", content_text(&v)));
    }
    let n = out.chars().count();
    if n > MAX_TRANSCRIPT_CHARS {
        let skip = n - MAX_TRANSCRIPT_CHARS;
        out = out.chars().skip(skip).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::FakeLlm;

    struct Failing;
    impl ChatClient for Failing {
        fn stream(&self, _: &ChatLedger, _: &mut dyn FnMut(&str)) -> Result<String> {
            Err(AppError::new(ErrorCode::LlmProvider))
        }
    }

    fn convo(turns: usize) -> ChatLedger {
        let mut l = ChatLedger::new();
        l.append_system("sys");
        l.append_user("<folder>ctx</folder> first question");
        l.append_assistant("first answer");
        for i in 0..turns {
            l.append_user(&format!("question {i} {}", "x".repeat(400)));
            l.append_assistant(&format!("answer {i} {}", "y".repeat(400)));
        }
        l
    }

    #[test]
    fn threshold_picks_none_soft_hard() {
        let l = convo(10);
        let t = estimate_tokens(&l);
        let p = CompactPolicy {
            soft_tokens: t + 10,
            hard_tokens: t + 1000,
            keep_recent_turns: 4,
        };
        assert_eq!(p.need(&l, "{}"), CompactNeed::None);
        let p = CompactPolicy {
            soft_tokens: t - 10,
            hard_tokens: t + 1000,
            keep_recent_turns: 4,
        };
        assert_eq!(p.need(&l, "{}"), CompactNeed::Soft);
        let big = "z".repeat(10_000);
        assert_eq!(p.need(&l, &big), CompactNeed::Hard);
        let w = CompactPolicy::for_window(100_000);
        assert_eq!((w.soft_tokens, w.hard_tokens), (60_000, 85_000));
    }

    #[test]
    fn compaction_keeps_head_and_recent_turns_verbatim_and_prefix_stable_until_then() {
        let mut l = convo(8);
        let before = l.to_json_array();
        // appending never touches the prefix
        l.append_user("q+");
        assert!(l.to_json_array().starts_with(&before[..before.len() - 1]));
        let old = l.clone();
        let c = compact_ledger(
            &l,
            &CompactPolicy::for_window(1000),
            Some(&FakeLlm("SUMMARY TEXT".into())),
        )
        .unwrap();
        let (a, b) = (old.messages(), c.messages());
        assert_eq!(a[..2], b[..2], "system + folder context turn unchanged");
        assert!(b[2].contains("SUMMARY TEXT") && b[2].contains("assistant"));
        // last 4 user turns (and their answers) verbatim
        assert_eq!(a[a.len() - 7..], b[b.len() - 7..]);
        assert_eq!(c.len(), 3 + 7);
        assert_eq!(c.compactions().len(), 1);
        let ev = &c.compactions()[0];
        assert!(ev.summarized && ev.tokens_after < ev.tokens_before);
        // after the compaction the new prefix is append-only again
        let mut c2 = c.clone();
        let p = c2.to_json_array();
        c2.append_assistant("a+");
        assert!(c2.to_json_array().starts_with(&p[..p.len() - 1]));
    }

    #[test]
    fn nothing_to_compact_in_a_short_conversation() {
        assert!(compact_ledger(&convo(2), &CompactPolicy::for_window(1000), None).is_none());
    }

    #[test]
    fn failed_summary_falls_back_to_truncation() {
        let l = convo(8);
        let c = compact_ledger(&l, &CompactPolicy::for_window(1000), Some(&Failing)).unwrap();
        assert!(c.messages()[2].contains("omitted"));
        assert!(!c.compactions()[0].summarized);
        assert_eq!(l.messages()[l.len() - 8..], c.messages()[c.len() - 8..]);
        let c = compact_ledger(&l, &CompactPolicy::for_window(1000), None).unwrap();
        assert!(!c.compactions()[0].summarized);
    }

    #[test]
    fn plan_survives_appends_after_the_snapshot() {
        let base = convo(8);
        let plan = CompactPlan::new(&base, 4).unwrap();
        let mut now = base.clone();
        now.append_user("late");
        now.append_assistant("late a");
        assert!(plan.still_applies(&base, &now));
        let c = plan.apply(&now, Some("S"));
        assert_eq!(c.messages().last(), now.messages().last());
        let other = convo(9);
        assert!(
            !plan.still_applies(&base, &other)
                || other.messages()[plan.upto - 1] == base.messages()[plan.upto - 1]
        );
    }

    #[test]
    fn images_are_not_counted_by_their_base64_length_and_become_markers() {
        let big = "A".repeat(500_000);
        let m = serde_json::json!({"role":"user","content":[
            {"type":"text","text":"look"},{"type":"image_url","image_url":{"url":format!("data:image/jpeg;base64,{big}")}}]})
        .to_string();
        assert!(estimate_message_tokens(&m) < 5000);
        assert!(transcript_of(&[m]).contains("[screenshot omitted]"));
    }
}
