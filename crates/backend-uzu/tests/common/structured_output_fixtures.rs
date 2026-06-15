use std::{path::PathBuf, sync::Arc};

use backend_uzu::{
    session::{
        config::{DecodingConfig, GrammarConfig, RunConfig, SpeculatorConfig},
        parameter::{SamplingMethod, SamplingPolicy, SamplingSeed},
        types::{Input, Message, Output},
    },
    speculators::empty_speculator::EmptySpeculator,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::common::path::get_test_model_path;

const LONG_NO_THINKING_ENV: &str = "UZU_SO_LONG_LIMIT";
const THINKING_MODEL_ENV: &str = "THINKING_TEST_MODEL";

pub const NO_THINKING_LIMITS_DEFAULT: &[u64] = &[32, 128, 256, 512];
pub const WITH_THINKING_LIMITS: &[u64] = &[32, 128, 512, 1024];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    PlainAsyncCandidate,
    PlainForcedSync,
    StructuredCalendarSchema,
    StructuredBuiltinJson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scenario {
    pub name: &'static str,
    pub kind: ScenarioKind,
    pub enable_thinking: bool,
}

impl Scenario {
    pub fn with_tokens_limit(
        &self,
        tokens_limit: u64,
    ) -> RunConfig {
        let mut config = RunConfig::default()
            .tokens_limit(tokens_limit)
            .enable_thinking(self.enable_thinking)
            .sampling_policy(greedy_sampling_policy());
        if let Some(grammar_config) = self.grammar_config() {
            config = config.grammar_config(grammar_config);
        }
        config
    }

    pub fn decoding_config(&self) -> DecodingConfig {
        let config = DecodingConfig::default().with_sampling_seed(SamplingSeed::Custom(42));
        if self.is_plain_forced_sync() {
            config.with_speculator_config(SpeculatorConfig::new(1, Arc::new(EmptySpeculator {})))
        } else {
            config
        }
    }

    pub fn grammar_config(&self) -> Option<GrammarConfig> {
        match self.kind {
            ScenarioKind::PlainAsyncCandidate | ScenarioKind::PlainForcedSync => None,
            ScenarioKind::StructuredCalendarSchema => Some(calendar_schema_grammar()),
            ScenarioKind::StructuredBuiltinJson => Some(GrammarConfig::builtin_json()),
        }
    }

    pub fn is_plain_forced_sync(&self) -> bool {
        matches!(self.kind, ScenarioKind::PlainForcedSync)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub name: String,
    pub date: String,
    pub participants: Vec<String>,
    pub location: String,
    pub metadata: CalendarEventMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEventMetadata {
    pub timezone: String,
    pub duration_minutes: u32,
}

pub fn no_thinking_scenarios() -> [Scenario; 4] {
    [
        Scenario {
            name: "plain_no_thinking",
            kind: ScenarioKind::PlainAsyncCandidate,
            enable_thinking: false,
        },
        Scenario {
            name: "plain_forced_sync_no_thinking",
            kind: ScenarioKind::PlainForcedSync,
            enable_thinking: false,
        },
        Scenario {
            name: "structured_calendar_event_no_thinking",
            kind: ScenarioKind::StructuredCalendarSchema,
            enable_thinking: false,
        },
        Scenario {
            name: "structured_builtin_json_no_thinking",
            kind: ScenarioKind::StructuredBuiltinJson,
            enable_thinking: false,
        },
    ]
}

pub fn with_thinking_scenarios() -> [Scenario; 4] {
    [
        Scenario {
            name: "plain_with_thinking",
            kind: ScenarioKind::PlainAsyncCandidate,
            enable_thinking: true,
        },
        Scenario {
            name: "plain_forced_sync_with_thinking",
            kind: ScenarioKind::PlainForcedSync,
            enable_thinking: true,
        },
        Scenario {
            name: "structured_calendar_event_with_thinking",
            kind: ScenarioKind::StructuredCalendarSchema,
            enable_thinking: true,
        },
        Scenario {
            name: "structured_builtin_json_with_thinking",
            kind: ScenarioKind::StructuredBuiltinJson,
            enable_thinking: true,
        },
    ]
}

pub fn no_thinking_limits() -> Vec<u64> {
    let mut limits = NO_THINKING_LIMITS_DEFAULT.to_vec();
    if long_no_thinking_enabled() {
        limits.push(1024);
    }
    limits
}

pub fn with_thinking_limits() -> Vec<u64> {
    WITH_THINKING_LIMITS.to_vec()
}

pub fn model_path_for_thinking() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(THINKING_MODEL_ENV).ok()?);
    path.join("config.json").exists().then_some(path)
}

pub fn model_path_for_no_thinking() -> PathBuf {
    get_test_model_path()
}

pub fn benchmark_input() -> Input {
    Input::Messages(vec![
        Message::system("Extract event details as strict JSON.".to_string()),
        Message::user(
            "Alice, Bob, and Carol will meet for a product sync next Tuesday at 10:30 AM in Berlin. \
            The meeting lasts 45 minutes and uses timezone Europe/Berlin."
                .to_string(),
        ),
    ])
}

pub fn calendar_schema_grammar() -> GrammarConfig {
    GrammarConfig::json_schema_simple(calendar_event_schema_json())
}

pub fn extract_response_text(output: &Output) -> &str {
    output.text.parsed.response.as_deref().unwrap_or(output.text.original.as_str())
}

pub fn parse_calendar_event(output: &Output) -> Result<CalendarEvent, serde_json::Error> {
    serde_json::from_str(extract_response_text(output))
}

pub fn long_no_thinking_enabled() -> bool {
    std::env::var(LONG_NO_THINKING_ENV)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

pub fn greedy_sampling_policy() -> SamplingPolicy {
    SamplingPolicy::Custom {
        value: SamplingMethod::Greedy,
    }
}

pub fn calendar_event_schema_json() -> String {
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "name": { "type": "string" },
            "date": {
                "type": "string",
                "description": "ISO-like date or natural date expression from source text"
            },
            "participants": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1
            },
            "location": { "type": "string" },
            "metadata": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "timezone": { "type": "string" },
                    "duration_minutes": { "type": "integer", "minimum": 1 }
                },
                "required": ["timezone", "duration_minutes"]
            }
        },
        "required": ["name", "date", "participants", "location", "metadata"]
    });
    serde_json::to_string(&schema).expect("calendar event schema should serialize")
}

pub fn parse_json_object(output: &Output) -> Result<Value, serde_json::Error> {
    serde_json::from_str(extract_response_text(output))
}
