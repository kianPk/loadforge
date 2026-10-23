use crate::config::RunConfig;
use crate::metrics::Metrics;
use anyhow::{anyhow, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::{Client, Method};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};
use uuid::Uuid;

pub struct Engine {
    metrics: Arc<Metrics>,
    stop: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

impl Engine {
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self {
            metrics,
            stop: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub async fn run(self: Arc<Self>, config: RunConfig) -> Result<()> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(anyhow!("a forge run is already in progress"));
        }

        self.stop.store(false, Ordering::Relaxed);
        let config = config.sanitize();
        let run_id = Uuid::new_v4().to_string();
        self.metrics.reset(run_id.clone(), config.target.clone());

        let client = Client::builder()
            .pool_max_idle_per_host(config.concurrency as usize)
            .timeout(config.timeout())
            .user_agent(format!("loadforge/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        let method = Method::from_bytes(config.method.as_bytes())
            .unwrap_or(Method::GET);

        if config.warmup_secs > 0 {
            info!(secs = config.warmup_secs, "warmup");
            let warm_end = Instant::now() + Duration::from_secs(config.warmup_secs);
            while Instant::now() < warm_end && !self.stop.load(Ordering::Relaxed) {
                let _ = fire_once(&client, &method, &config).await;
                sleep(Duration::from_millis(20)).await;
            }
            // discard warmup stats
            self.metrics.reset(run_id.clone(), config.target.clone());
        }

        let deadline = Instant::now() + config.duration();
        let sem = Arc::new(Semaphore::new(config.concurrency as usize));
        let mut tasks = FuturesUnordered::new();
        let interval = config
            .rps
            .map(|rps| Duration::from_secs_f64(1.0 / rps as f64));

        info!(
            target = %config.target,
            concurrency = config.concurrency,
            duration_secs = config.duration_secs,
            "forging"
        );

        let mut next_slot = Instant::now();
        while Instant::now() < deadline && !self.stop.load(Ordering::Relaxed) {
            if let Some(iv) = interval {
                let now = Instant::now();
                if now < next_slot {
                    sleep(next_slot - now).await;
                }
                next_slot += iv;
            }

            let permit = match sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => break,
            };

            let client = client.clone();
            let method = method.clone();
            let cfg = config.clone();
            let metrics = self.metrics.clone();
            let stop = self.stop.clone();

            tasks.push(tokio::spawn(async move {
                let _permit = permit;
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                metrics.begin_request();
                let started = Instant::now();
                match fire_once(&client, &method, &cfg).await {
                    Ok((status, bytes)) => {
                        metrics.record_ok(status, started.elapsed(), bytes);
                    }
                    Err(_) => {
                        metrics.record_err(started.elapsed());
                    }
                }
                metrics.end_request();
            }));

            // keep the join set from growing unbounded when rps is high
            while tasks.len() > (config.concurrency as usize) * 4 {
                let _ = tasks.next().await;
            }
        }

        while let Some(res) = tasks.next().await {
            if let Err(e) = res {
                warn!(error = %e, "worker join error");
            }
        }

        self.metrics.finish();
        self.busy.store(false, Ordering::Relaxed);
        info!(run_id = %run_id, "forge complete");
        Ok(())
    }
}

async fn fire_once(
    client: &Client,
    method: &Method,
    config: &RunConfig,
) -> Result<(u16, u64)> {
    let mut req = client.request(method.clone(), &config.target);
    for (k, v) in &config.headers {
        req = req.header(k, v);
    }
    if let Some(body) = &config.body {
        req = req.body(body.clone());
    }

    let fut = async {
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await?.len() as u64;
        Ok::<_, reqwest::Error>((status, bytes))
    };

    match timeout(config.timeout(), fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(anyhow!(e)),
        Err(_) => Err(anyhow!("request timed out")),
    }
}
