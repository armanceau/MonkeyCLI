use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Clone, Debug)]
pub struct OllamaClient {
    host: String,
    client: reqwest::Client,
}

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: Option<AssistantMessage>,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    message: Option<AssistantMessage>,
    done: Option<bool>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

impl OllamaClient {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.host)
    }

    fn tags_url(&self) -> String {
        format!("{}/api/tags", self.host)
    }

    pub async fn chat(&self, model: &str, messages: Vec<Message>, stream: bool) -> Result<String> {
        let response = self
            .client
            .post(self.chat_url())
            .json(&ChatRequest {
                model,
                messages: &messages,
                stream,
            })
            .send()
            .await
            .with_context(|| format!("failed to send request to Ollama at {}", self.host))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("ollama returned {}: {}", status, body));
        }

        if !stream {
            let data: ChatResponse = response.json().await.context("invalid Ollama chat response")?;
            return Ok(data.message.and_then(|message| message.content).unwrap_or_default());
        }

        let mut output = String::new();
        let mut buffer = String::new();
        let mut bytes = response.bytes_stream();

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(index) = buffer.find('\n') {
                let line = buffer[..index].trim().to_string();
                buffer = buffer[index + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                let chunk: StreamChunk = serde_json::from_str(&line).context("invalid Ollama stream chunk")?;
                if let Some(message) = chunk.message {
                    if let Some(content) = message.content {
                        print!("{content}");
                        std::io::stdout().flush()?;
                        output.push_str(&content);
                    }
                }

                if chunk.done == Some(true) {
                    break;
                }
            }
        }

        if !buffer.trim().is_empty() {
            let chunk: StreamChunk = serde_json::from_str(buffer.trim()).context("invalid Ollama tail chunk")?;
            if let Some(message) = chunk.message {
                if let Some(content) = message.content {
                    print!("{content}");
                    std::io::stdout().flush()?;
                    output.push_str(&content);
                }
            }
        }

        Ok(output)
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let response = self.client.get(self.tags_url()).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("ollama returned {}: {}", status, body));
        }

        let data: TagsResponse = response.json().await?;
        Ok(data.models.into_iter().map(|model| model.name).collect())
    }

    pub async fn first_local_model(&self) -> Result<Option<String>> {
        Ok(self.list_models().await?.into_iter().next())
    }
}
