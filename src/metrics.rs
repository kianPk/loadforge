use parking_lot::Mutex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const BUCKETS_MS: &[u64] = &[
    1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10_000,
    15_000, 30_000, 60_000,
];

#[derive(Default)]
struct LatencyHist {
    counts: BTreeMap<u64, u64>,
    total: u64,
}

impl LatencyHist {
    fn record(&mut self, ms: u64) {
        let bucket = BUCKETS_MS
            .iter()
            .copied()
            .find(|b| ms <= *b)
            .unwrap_or(u64::MAX);
        *self.counts.entry(bucket).or_default() += 1;
        self.total += 1;
    }

    fn percentile(&self, p: f64) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let target = ((self.total as f64) * p).ceil().max(1.0) as u64;
        let mut acc = 0u64;
        for (bucket, count) in &self.counts {
            acc += count;
            if acc >= target {
                return if *bucket == u64::MAX {
                    60_000.0
                } else {
                    *bucket as f64
                };
            }
        }
        0.0
    }

    fn mean_approx(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for (bucket, count) in &self.counts {
            let mid = if *bucket == u64::MAX {
                60_000.0
            } else {
                *bucket as f64 * 0.75
            };
            sum += mid * (*count as f64);
        }
        sum / self.total as f64
    }
}

pub struct Metrics {
    started: Mutex<Instant>,
    pub running: AtomicBool,
    ok: AtomicU64,
    err: AtomicU64,
    bytes: AtomicU64,
    in_flight: AtomicU64,
    status_2xx: AtomicU64,
    status_3xx: AtomicU64,
    status_4xx: AtomicU64,
    status_5xx: AtomicU64,
    status_other: AtomicU64,
    hist: Mutex<LatencyHist>,
    recent: Mutex<Vec<f64>>,
    target: Mutex<String>,
    run_id: Mutex<String>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Mutex::new(Instant::now()),
            running: AtomicBool::new(false),
            ok: AtomicU64::new(0),
            err: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            status_2xx: AtomicU64::new(0),
            status_3xx: AtomicU64::new(0),
            status_4xx: AtomicU64::new(0),
            status_5xx: AtomicU64::new(0),
            status_other: AtomicU64::new(0),
            hist: Mutex::new(LatencyHist::default()),
            recent: Mutex::new(Vec::with_capacity(120)),
            target: Mutex::new(String::new()),
            run_id: Mutex::new(String::new()),
        }
    }
}

impl Metrics {
    pub fn reset(&self, run_id: impl Into<String>, target: impl Into<String>) {
        self.ok.store(0, Ordering::Relaxed);
        self.err.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
        self.in_flight.store(0, Ordering::Relaxed);
        self.status_2xx.store(0, Ordering::Relaxed);
        self.status_3xx.store(0, Ordering::Relaxed);
        self.status_4xx.store(0, Ordering::Relaxed);
        self.status_5xx.store(0, Ordering::Relaxed);
        self.status_other.store(0, Ordering::Relaxed);
        *self.hist.lock() = LatencyHist::default();
        self.recent.lock().clear();
        *self.target.lock() = target.into();
        *self.run_id.lock() = run_id.into();
        *self.started.lock() = Instant::now();
        self.running.store(true, Ordering::Relaxed);
    }

    pub fn begin_request(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    pub fn end_request(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_ok(&self, status: u16, latency: Duration, body_len: u64) {
        self.ok.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(body_len, Ordering::Relaxed);
        match status {
            200..=299 => {
                self.status_2xx.fetch_add(1, Ordering::Relaxed);
            }
            300..=399 => {
                self.status_3xx.fetch_add(1, Ordering::Relaxed);
            }
            400..=499 => {
                self.status_4xx.fetch_add(1, Ordering::Relaxed);
            }
            500..=599 => {
                self.status_5xx.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.status_other.fetch_add(1, Ordering::Relaxed);
            }
        }
        let ms = latency.as_secs_f64() * 1000.0;
        self.hist.lock().record(ms.max(0.0) as u64);
        let mut recent = self.recent.lock();
        recent.push(ms);
        if recent.len() > 240 {
            let drain = recent.len() - 240;
            recent.drain(0..drain);
        }
    }

    pub fn record_err(&self, latency: Duration) {
        self.err.fetch_add(1, Ordering::Relaxed);
        let ms = latency.as_secs_f64() * 1000.0;
        self.hist.lock().record(ms.max(0.0) as u64);
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> Snapshot {
        let ok = self.ok.load(Ordering::Relaxed);
        let err = self.err.load(Ordering::Relaxed);
        let total = ok + err;
        let elapsed = self.started.lock().elapsed().as_secs_f64().max(0.001);
        let hist = self.hist.lock();
        let recent = self.recent.lock().clone();
        Snapshot {
            run_id: self.run_id.lock().clone(),
            target: self.target.lock().clone(),
            running: self.running.load(Ordering::Relaxed),
            elapsed_secs: elapsed,
            ok,
            err,
            total,
            rps: total as f64 / elapsed,
            bytes: self.bytes.load(Ordering::Relaxed),
            in_flight: self.in_flight.load(Ordering::Relaxed),
            p50_ms: hist.percentile(0.50),
            p90_ms: hist.percentile(0.90),
            p95_ms: hist.percentile(0.95),
            p99_ms: hist.percentile(0.99),
            mean_ms: hist.mean_approx(),
            error_rate: if total == 0 {
                0.0
            } else {
                err as f64 / total as f64
            },
            error_budget_remaining: {
                // Soft SLO: keep error rate under 1%. Remaining is how much headroom is left.
                let rate = if total == 0 {
                    0.0
                } else {
                    err as f64 / total as f64
                };
                (0.01 - rate).clamp(-1.0, 0.01)
            },
            status_2xx: self.status_2xx.load(Ordering::Relaxed),
            status_3xx: self.status_3xx.load(Ordering::Relaxed),
            status_4xx: self.status_4xx.load(Ordering::Relaxed),
            status_5xx: self.status_5xx.load(Ordering::Relaxed),
            status_other: self.status_other.load(Ordering::Relaxed),
            recent_latencies_ms: recent,
            ts: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub run_id: String,
    pub target: String,
    pub running: bool,
    pub elapsed_secs: f64,
    pub ok: u64,
    pub err: u64,
    pub total: u64,
    pub rps: f64,
    pub bytes: u64,
    pub in_flight: u64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub mean_ms: f64,
    /// Fraction of requests that failed (0.0–1.0).
    pub error_rate: f64,
    /// Headroom vs a 1% error SLO (negative = over budget).
    pub error_budget_remaining: f64,
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub status_other: u64,
    pub recent_latencies_ms: Vec<f64>,
    pub ts: String,
}
