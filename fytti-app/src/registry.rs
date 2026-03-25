use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const REGISTRY_URL: &str = "http://localhost:9000/registry/announce";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Announce Fytti to Hermytt's service registry and heartbeat in the background.
/// Returns a handle that stops the heartbeat when dropped.
pub fn start(token: &str, apps: &[String]) -> RegistryHandle {
    let alive = Arc::new(AtomicBool::new(true));
    let alive_clone = alive.clone();
    let token = token.to_string();
    let apps = apps.to_vec();

    thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        while alive_clone.load(Ordering::Relaxed) {
            let body = serde_json::json!({
                "name": "fytti",
                "role": "renderer",
                "endpoint": "http://localhost:0",
                "meta": {
                    "apps_loaded": apps,
                    "gpu": "wgpu"
                }
            });

            let _ = client
                .post(REGISTRY_URL)
                .header("X-Hermytt-Key", &token)
                .json(&body)
                .send();

            thread::sleep(HEARTBEAT_INTERVAL);
        }
    });

    RegistryHandle { alive }
}

pub struct RegistryHandle {
    alive: Arc<AtomicBool>,
}

impl Drop for RegistryHandle {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}
