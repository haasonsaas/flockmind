use anyhow::{anyhow, Result};
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs, ResponseFormat,
    },
    Client,
};

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_key: String,
    pub api_base: Option<String>,
    pub model: String,
    pub max_tokens: u16,
    pub temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_base: None,
            model: "gpt-4o-mini".to_string(),
            max_tokens: 2048,
            temperature: 0.1,
        }
    }
}

pub struct LlmClient {
    client: Client<OpenAIConfig>,
    config: LlmConfig,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let mut openai_config = OpenAIConfig::new().with_api_key(&config.api_key);

        if let Some(ref base) = config.api_base {
            openai_config = openai_config.with_api_base(base);
        }

        let client = Client::with_config(openai_config);

        Ok(Self { client, config })
    }

    pub async fn chat(&self, system: &str, user: &str) -> Result<String> {
        let messages = vec![
            ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(system)
                    .build()?,
            ),
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(user)
                    .build()?,
            ),
        ];

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.config.model)
            .messages(messages)
            .max_tokens(self.config.max_tokens)
            .temperature(self.config.temperature)
            .response_format(ResponseFormat::JsonObject)
            .build()?;

        let response = self.client.chat().create(request).await?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| anyhow!("No response content from LLM"))?;

        Ok(content)
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &LlmConfig {
        &self.config
    }
}
