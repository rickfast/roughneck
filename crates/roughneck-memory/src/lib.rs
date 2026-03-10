use async_trait::async_trait;
use roughneck_core::{MemoryBackend, MemoryEvent, Result};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct InMemoryMemoryBackend {
    events: tokio::sync::RwLock<HashMap<String, Vec<MemoryEvent>>>,
}

#[async_trait]
impl MemoryBackend for InMemoryMemoryBackend {
    async fn append_event(&self, conv_id: &str, event: MemoryEvent) -> Result<()> {
        let mut guard = self.events.write().await;
        guard.entry(conv_id.to_string()).or_default().push(event);
        Ok(())
    }

    async fn get_events(&self, conv_id: &str, limit: usize) -> Result<Vec<MemoryEvent>> {
        let guard = self.events.read().await;
        let Some(events) = guard.get(conv_id) else {
            return Ok(Vec::new());
        };
        if limit == 0 {
            return Ok(Vec::new());
        }
        let start = events.len().saturating_sub(limit);
        Ok(events[start..].to_vec())
    }

    async fn search(&self, conv_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEvent>> {
        let guard = self.events.read().await;
        let Some(events) = guard.get(conv_id) else {
            return Ok(Vec::new());
        };

        let needle = query.to_lowercase();
        let mut matches = Vec::new();
        for event in events.iter().rev() {
            if event.payload.to_string().to_lowercase().contains(&needle) {
                matches.push(event.clone());
            }
            if matches.len() >= limit {
                break;
            }
        }
        matches.reverse();
        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roughneck_core::{MemoryScope, now_millis};
    use serde_json::json;

    #[tokio::test]
    async fn append_and_search() {
        let backend = InMemoryMemoryBackend::default();
        backend
            .append_event(
                "conv",
                MemoryEvent {
                    scope: MemoryScope::ShortTerm,
                    kind: "message".to_string(),
                    payload: json!({"content": "hello rig"}),
                    timestamp_ms: now_millis(),
                },
            )
            .await
            .unwrap();

        let found = backend.search("conv", "rig", 5).await.unwrap();
        assert_eq!(found.len(), 1);
    }
}
