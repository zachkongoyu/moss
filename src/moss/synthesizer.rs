use crate::{moss::blackboard, providers::{DynProvider, Message, Role}};
use serde_json::Value;

use super::blackboard::{Blackboard, Evidence, GapState};

pub(in crate::moss) struct Synthesizer {
    provider: DynProvider
}

impl Synthesizer {
    fn new(provider: DynProvider) -> Self {
        Self { provider }
    }

    fn synthesize(&self, query: Box<str>, blackboard: &Blackboard) -> Value {
        let template = std::fs::read_to_string("src/moss/prompts/synthesizer.xml")
            .expect("synthesizer prompt file missing: src/moss/prompts/synthesizer.xml");

        let blackboard_state = serde_json::to_string(blackboard).unwrap();

        let rendered = template
            .replace("{user_query}", &query)
            .replace("{blackboard_state}", &blackboard_state);

        let response = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(self.provider.complete_chat(vec![Message { role: Role::User, content: rendered.into_boxed_str() }]));
        response
    }
}
