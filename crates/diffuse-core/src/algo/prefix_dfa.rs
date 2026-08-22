//! Direct Table DFA and Aho-Corasick Pattern Matching Automata.
//!
//! Compiles dictionary prefixes and language boundary tokens into a 256-state
//! direct lookup transition table. Matches longest prefix in O(1) time per byte
//! with zero backtracking and zero heap allocation during search.

use super::PrefixMatcher;

const DEAD_STATE: u32 = 0;

/// Direct lookup 256-way table DFA for high-performance prefix matching.
#[derive(Clone, Debug)]
pub struct PrefixDfa {
    /// Flattened transition table: transitions[state * 256 + byte] ->
    /// next_state
    transitions: Vec<u32>,
    /// Terminal match length for each state (0 if non-terminal)
    match_lengths: Vec<usize>,
}

impl PrefixDfa {
    /// Construct a direct DFA from an iterator of prefix pattern strings.
    pub fn from_patterns<I, S>(patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        // State 0 is DEAD_STATE, State 1 is ROOT_STATE
        let mut transitions = vec![DEAD_STATE; 256 * 2];
        let mut match_lengths = vec![0; 2];
        let root_state = 1u32;

        for pattern in patterns {
            let bytes = pattern.as_ref().as_bytes();
            if bytes.is_empty() {
                continue;
            }

            let mut curr = root_state;
            for &b in bytes {
                let idx = (curr as usize) * 256 + (b as usize);
                let next = transitions[idx];
                if next != DEAD_STATE {
                    curr = next;
                } else {
                    let new_state = (transitions.len() / 256) as u32;
                    transitions.extend_from_slice(&[DEAD_STATE; 256]);
                    match_lengths.push(0);
                    transitions[idx] = new_state;
                    curr = new_state;
                }
            }

            let curr_idx = curr as usize;
            match_lengths[curr_idx] = match_lengths[curr_idx].max(bytes.len());
        }

        Self {
            transitions,
            match_lengths,
        }
    }

    /// Check if the text starts with any prefix registered in the DFA.
    #[must_use]
    pub fn has_matching_prefix(&self, text: &str) -> bool {
        self.longest_matching_prefix(text).is_some()
    }

    /// Find all prefixes registered in the DFA that match the start of `text`.
    #[must_use]
    pub fn all_matching_prefixes<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let bytes = text.as_bytes();
        let mut matches = Vec::new();
        let mut curr = 1u32; // ROOT_STATE

        for (idx, &b) in bytes.iter().enumerate() {
            let next = self.transitions[(curr as usize) * 256 + (b as usize)];
            if next == DEAD_STATE {
                break;
            }
            curr = next;
            let len = self.match_lengths[curr as usize];
            if len > 0 && len == idx + 1 {
                matches.push(&text[..len]);
            }
        }

        matches
    }
}

impl PrefixMatcher for PrefixDfa {
    fn longest_matching_prefix<'a>(&self, text: &'a str) -> Option<&'a str> {
        let bytes = text.as_bytes();
        let mut longest_len = None;
        let mut curr = 1u32; // ROOT_STATE

        for (idx, &b) in bytes.iter().enumerate() {
            let next = self.transitions[(curr as usize) * 256 + (b as usize)];
            if next == DEAD_STATE {
                break;
            }
            curr = next;
            let len = self.match_lengths[curr as usize];
            if len > 0 && len == idx + 1 {
                longest_len = Some(len);
            }
        }

        longest_len.map(|len| &text[..len])
    }
}
