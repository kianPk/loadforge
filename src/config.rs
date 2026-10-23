use serde::{Deserialize, Serialize};
// Deserialize used for --scenario JSON files
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunConfig {
    pub target: String,
    pub method: String,
    pub concurrency: u32,
    pub duration_secs: u64,
    pub rps: Option<u32>,
    pub timeout_ms: u64,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub warmup_secs: u64,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            target: "https://httpbin.org/get".into(),
            method: "GET".into(),
            concurrency: 32,
            duration_secs: 15,
            rps: None,
            timeout_ms: 5_000,
            headers: HashMap::new(),
            body: None,
            warmup_secs: 0,
        }
    }
}

impl RunConfig {
    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.duration_secs.max(1))
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.max(100))
    }

    pub fn sanitize(mut self) -> Self {
        self.concurrency = self.concurrency.clamp(1, 10_000);
        self.duration_secs = self.duration_secs.clamp(1, 3_600);
        self.timeout_ms = self.timeout_ms.clamp(100, 120_000);
        self.warmup_secs = self.warmup_secs.min(60);
        if let Some(rps) = self.rps {
            self.rps = Some(rps.clamp(1, 200_000));
        }
        let method = self.method.trim().to_uppercase();
        self.method = if method.is_empty() {
            "GET".into()
        } else {
            method
        };
        self
    }
}
