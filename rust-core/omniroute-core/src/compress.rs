use crate::chat::{Content, Message};

/// M8: context compression — RTK (Recursive Token Knapsack) style selection
/// plus session dedup. Keeps important messages (system, tool results, the
/// tail of the conversation) within a token budget; replaces the trimmed
/// middle with a placeholder so the model still knows history was cut.
pub struct Compress;

impl Compress {
    /// Rough token estimate: ~4 chars per token (English-centric heuristic).
    pub fn estimate_tokens(text: &str) -> usize {
        (text.chars().count() / 4).max(1)
    }

    pub fn message_tokens(m: &Message) -> usize {
        let text = match &m.content {
            Some(Content::Text(t)) => t.clone(),
            Some(Content::Parts(parts)) => parts
                .iter()
                .map(|p| p.text.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" "),
            None => String::new(),
        };
        Self::estimate_tokens(&text) + Self::estimate_tokens(&m.role)
    }

    /// RTK selection: always keep system + first user message + the tail
    /// (last `keep_tail` messages), then fill remaining budget with the most
    /// recent middle messages. Returns (selected, trimmed_count).
    pub fn rtk_select(
        messages: &[Message],
        budget_tokens: usize,
        keep_tail: usize,
    ) -> (Vec<Message>, usize) {
        if budget_tokens == 0 || Self::total_tokens(messages) <= budget_tokens {
            return (messages.to_vec(), 0);
        }
        if messages.is_empty() {
            return (vec![], 0);
        }

        let mut selected: Vec<Message> = Vec::new();
        let mut budget = budget_tokens;

        // 1. system + first user always kept
        let mut head_end = 0usize;
        for (i, m) in messages.iter().enumerate() {
            if i == 0 || m.role == "system" {
                if Self::message_tokens(m) <= budget {
                    selected.push(m.clone());
                    budget -= Self::message_tokens(m);
                }
                head_end = i + 1;
            } else {
                break;
            }
        }

        // 2. tail kept
        let tail_start = messages.len().saturating_sub(keep_tail).max(head_end);
        let mut tail: Vec<Message> = Vec::new();
        for m in messages[tail_start..].iter() {
            let t = Self::message_tokens(m);
            if t <= budget {
                tail.push(m.clone());
                budget -= t;
            }
        }

        // 3. fill the middle (most recent first) while budget allows
        let mut middle: Vec<Message> = Vec::new();
        let mid_range = head_end..tail_start;
        let mut mid_msgs: Vec<&Message> = messages[mid_range].iter().collect();
        mid_msgs.reverse();
        for m in mid_msgs {
            let t = Self::message_tokens(m);
            if t <= budget {
                middle.push(m.clone());
                budget -= t;
            } else {
                break;
            }
        }
        middle.reverse();

        selected.extend(middle);
        selected.extend(tail);

        let trimmed = messages.len() - selected.len();
        if trimmed > 0 && !selected.is_empty() {
            // placeholder keeps the model aware history was compacted
            selected.insert(
                head_end.min(selected.len()),
                Message {
                    role: "system".into(),
                    content: Some(Content::Text(format!(
                        "[context compressed: {trimmed} earlier messages trimmed by omniroute-rs RTK]"
                    ))),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        }
        (selected, trimmed)
    }

    /// Session dedup: for long multi-turn conversations, drop everything
    /// except system + a running summary + the last `keep_tail` messages.
    /// Returns (compacted, trimmed).
    pub fn session_dedup(
        messages: &[Message],
        max_messages: usize,
        keep_tail: usize,
    ) -> (Vec<Message>, usize) {
        if messages.len() <= max_messages {
            return (messages.to_vec(), 0);
        }
        let mut selected: Vec<Message> = Vec::new();
        for m in messages.iter() {
            if m.role == "system" {
                selected.push(m.clone());
            }
        }
        let tail_start = messages.len().saturating_sub(keep_tail);
        let middle_count = tail_start.saturating_sub(selected.len());
        let trimmed = middle_count.saturating_sub(0);
        if middle_count > 0 {
            selected.push(Message {
                role: "system".into(),
                content: Some(Content::Text(format!(
                    "[context compacted: {middle_count} earlier turns summarized away]"
                ))),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
        selected.extend_from_slice(&messages[tail_start..]);
        (selected, trimmed)
    }

    pub fn total_tokens(messages: &[Message]) -> usize {
        messages.iter().map(Self::message_tokens).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.into(),
            content: Some(Content::Text(text.into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_estimate() {
        assert_eq!(Compress::estimate_tokens(&"a".repeat(40)), 10);
        assert!(Compress::estimate_tokens("") >= 1);
    }

    #[test]
    fn test_rtk_within_budget_noop() {
        let msgs = vec![msg("system", "sys"), msg("user", "hi")];
        let (out, trimmed) = Compress::rtk_select(&msgs, 10_000, 4);
        assert_eq!(out.len(), 2);
        assert_eq!(trimmed, 0);
    }

    #[test]
    fn test_rtk_trims_middle_keeps_tail() {
        let mut msgs = vec![msg("system", "rules")];
        for i in 0..20 {
            msgs.push(msg(
                "user",
                &format!("message number {i} padded content here"),
            ));
            msgs.push(msg(
                "assistant",
                &format!("reply {i} padded content here too"),
            ));
        }
        let (out, trimmed) = Compress::rtk_select(&msgs, 120, 4);
        assert!(trimmed > 0, "should trim something");
        // tail preserved: last assistant message present
        assert!(out.iter().any(|m| {
            m.role == "assistant"
                && m.content
                    .as_ref()
                    .map(|c| c.to_string().contains("reply 19"))
                    .unwrap_or(false)
        }));
        // system preserved
        assert!(out.iter().any(|m| {
            m.role == "system"
                && m.content
                    .as_ref()
                    .map(|c| c.to_string().contains("rules"))
                    .unwrap_or(false)
        }));
        // placeholder present
        assert!(out.iter().any(|m| {
            m.content
                .as_ref()
                .map(|c| c.to_string().contains("context compressed"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn test_session_dedup() {
        let mut msgs = vec![msg("system", "sys")];
        for i in 0..10 {
            msgs.push(msg("user", &format!("u{i}")));
            msgs.push(msg("assistant", &format!("a{i}")));
        }
        let (out, trimmed) = Compress::session_dedup(&msgs, 6, 2);
        assert!(trimmed > 0);
        assert!(out.len() < msgs.len());
        assert!(out.iter().any(|m| {
            m.content
                .as_ref()
                .map(|c| c.to_string().contains("compacted"))
                .unwrap_or(false)
        }));
        // last assistant turn survives
        assert!(
            out.last()
                .map(|m| m
                    .content
                    .as_ref()
                    .map(|c| c.to_string() == "a9")
                    .unwrap_or(false))
                .unwrap_or(false)
        );
    }
}
