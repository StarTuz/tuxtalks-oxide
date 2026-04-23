//! LLM intent extractor — **feature-gated and pending rework**.
//!
//! This module asks an LLM (via SpeechD-NG `think`) to classify a voice command
//! into one of a fixed enum of [`Intent`] variants. That is **not** how the
//! reference Python app decides commands. Python uses `text_normalizer`,
//! `command_processor`, and `voice_fingerprint` — deterministic rule-based
//! routing plus learned phonetic corrections, not an LLM classifier. Keep this
//! module off (`--features voice` is not default) until the Rust voice path is
//! reimplemented to match Python.

use serde::Deserialize;
use std::fmt::Write as _;
use thiserror::Error;

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "intent", content = "parameters")]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    PlayArtist { artist: String },
    PlayAlbum { album: String },
    PlayTrack { track: String },
    PlayGenre { genre: String },
    PlayPlaylist { name: String },
    VolumeUp {},
    VolumeDown {},
    NextTrack {},
    PreviousTrack {},
    Pause {},
    Resume {},
    Stop {},
    WhatIsPlaying {},
    GameCommand { command: String },
    Unknown {},
}

#[derive(Debug, Error)]
pub enum IntentError {
    #[error("Failed to parse AI response: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("AI returned empty response")]
    EmptyResponse,
}

pub struct IntentEngine;

impl Default for IntentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentEngine {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn construct_prompt(text: &str, library_context: Option<&str>) -> String {
        let mut prompt = String::from(
            r#"You are a voice command interpreter for a media player and gaming voice assistant.
Extract the intent and parameters from the following voice command.

IMPORTANT: This text came from speech recognition and may contain errors.
Common ASR mistakes you should correct:
- "back oven" → "beethoven"
- "told them" → "beethoven"
- "cargo soup" → "cargo scoop"
- "landing girl" → "landing gear"
- "hardpoint" variations → "hardpoints"
- "track gale" → "gear" (landing gear)
- "play over" → "play beethoven"
- "she economy" → "kiri te kanawa"

Valid Intents:
- play_artist: Play music by an artist (parameters: {"artist": "string"})
- play_album: Play a specific album (parameters: {"album": "string"})
- play_track: Play a specific track (parameters: {"track": "string"})
- play_genre: Play music by genre (parameters: {"genre": "string"})
- play_playlist: Play a specific playlist (parameters: {"name": "string"})
- volume_up: Increase volume
- volume_down: Decrease volume
- next_track: Skip to next track
- previous_track: Go to previous track
- pause: Pause playback
- resume: Resume playback
- stop: Stop playback
- what_is_playing: Query current track
- game_command: Execute game action (ONLY if clearly game-related like "gear up", "dock")
"#,
        );

        if let Some(ctx) = library_context {
            prompt.push_str("\nUSER'S MUSIC LIBRARY (correct ASR errors to match these):\n");
            prompt.push_str(ctx);
            prompt.push('\n');
        }

        let _ = write!(
            prompt,
            "\nVoice Command: \"{text}\"\n\nRespond with JSON only:\n{{\"intent\": \"intent_name\", \"parameters\": {{}}, \"confidence\": 0.95}}"
        );
        prompt
    }

    /// Parse an LLM JSON response into an [`Intent`].
    ///
    /// # Errors
    /// Returns an [`IntentError`] if the response is empty, contains no
    /// parseable JSON object, or is malformed.
    pub fn parse_response(response: &str) -> Result<Intent, IntentError> {
        let trimmed = response.trim();
        if trimmed.is_empty() {
            return Err(IntentError::EmptyResponse);
        }

        // Implementation of a balanced-brace scanner to find ALL JSON objects.
        // This handles nested objects and selecting the valid intent from wrappers.
        let mut stack = Vec::new();
        let mut in_string = false;
        let mut escaped = false;
        let mut candidates = Vec::new();

        for (i, c) in trimmed.char_indices() {
            // Handle strings and escape characters
            if c == '"' && !escaped {
                in_string = !in_string;
            }
            if in_string {
                if c == '\\' {
                    escaped = !escaped;
                } else {
                    escaped = false;
                }
                continue;
            }

            // Track braces
            if c == '{' {
                stack.push(i);
            } else if c == '}' {
                if let Some(start) = stack.pop() {
                    candidates.push(&trimmed[start..=i]);
                }
            }
        }

        // Sort candidates by length descending (prefer outer-most objects)
        candidates.sort_by_key(|c| std::cmp::Reverse(c.len()));

        // Also add the raw trimmed string as a fallback candidate if it starts with {
        if trimmed.starts_with('{') && !candidates.contains(&trimmed) {
            candidates.push(trimmed);
        }

        for json_str in candidates {
            if let Ok(intent) = serde_json::from_str::<Intent>(json_str) {
                return Ok(intent);
            }
        }

        Err(IntentError::EmptyResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_parse_markdown_json() {
        let response =
            "Here is the intent:\n```json\n{\"intent\": \"pause\", \"parameters\": {}}\n```";
        let intent = IntentEngine::parse_response(response).unwrap();
        match intent {
            Intent::Pause {} => (),
            _ => panic!("Expected Pause intent"),
        }
    }

    #[test]
    fn test_parse_raw_json() {
        let response = "{\"intent\": \"volume_up\", \"parameters\": {}}";
        let intent = IntentEngine::parse_response(response).unwrap();
        match intent {
            Intent::VolumeUp {} => (),
            _ => panic!("Expected VolumeUp intent"),
        }
    }

    proptest! {
        #[test]
        fn test_parse_fuzzy_json(s in ".*") {
            // This test just ensures we don't crash on random input
            let _ = IntentEngine::parse_response(&s);
        }

        #[test]
        fn test_parse_valid_json_with_garbage(prefix in ".*", suffix in ".*") {
            let json = "{\"intent\": \"stop\", \"parameters\": {}}";
            let response = format!("{prefix}{json}{suffix}");
            // If the garbage doesn't contain '{' or '}', our simple extractor should find the JSON
            if !prefix.contains('{') && !suffix.contains('}') {
                if let Ok(intent) = IntentEngine::parse_response(&response) {
                    match intent {
                        Intent::Stop {} => (),
                        _ => panic!("Expected Stop intent"),
                    }
                }
            }
        }
    }

    #[test]
    fn test_nested_intent_wrapper() {
        let response = r#"Here is the JSON: {"reasoning": "playing music", "action": {"intent": "play_artist", "parameters": {"artist": "Bach"}}}"#;
        let intent = IntentEngine::parse_response(response).unwrap();
        match intent {
            Intent::PlayArtist { artist } => assert_eq!(artist, "Bach"),
            _ => panic!("Expected PlayArtist"),
        }
    }

    #[test]
    fn test_multiple_candidates_first_valid() {
        // First one is valid JSON but not an Intent. Second one is valid Intent.
        let response =
            r#"I found this: {"foo": "bar"} and also this: {"intent": "pause", "parameters": {}}"#;
        let intent = IntentEngine::parse_response(response).unwrap();
        match intent {
            Intent::Pause {} => (),
            _ => panic!("Expected Pause"),
        }
    }

    #[test]
    fn test_malformed_then_valid() {
        // First block is balanced but invalid JSON.
        let response =
            r#"Some junk { "garbage" } then real intent {"intent": "stop", "parameters": {}}"#;
        let intent = IntentEngine::parse_response(response).unwrap();
        match intent {
            Intent::Stop {} => (),
            _ => panic!("Expected Stop"),
        }
    }
}
