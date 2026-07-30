//! Local sidecar supervision for the Mini-Me Python backend.
//!
//! The desktop app is a *client* of the existing Mini-Me agent stack, not a
//! reimplementation of it. `BackendSupervisor` owns the lifecycle of a locally
//! spawned backend process: it starts it on a localhost port, waits for health,
//! streams turns over HTTP/SSE, and tears it down on quit. Running the backend
//! locally is what lets the app inherit the local `asta` CLI's auto-refreshing
//! auth (killing the web app's token-expiry pain).
//!
//! This is a **stub** for P6.0 — the process spawn and health-check are sketched
//! against a dev backend (assume `uv`/venv on PATH); real streaming lands in
//! P6.2. Nothing here runs a subagent; org policy stays human-gated.

// The supervisor is constructed in `main` but its methods aren't called until
// P6.2 wires the sidecar lifecycle in. Silence dead-code for this forward-looking
// scaffolding rather than deleting code we're about to use.
#![allow(dead_code)]

use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

/// How the client reaches the backend. Defaults to a locally spawned sidecar;
/// a hosted URL is a future fallback (not the chosen direction).
#[derive(Clone, Debug)]
pub struct BackendConfig {
    /// Port the local sidecar listens on.
    pub port: u16,
    /// Working directory of the Mini-Me checkout to launch from.
    pub project_dir: String,
    /// Command + args that start the dev backend (e.g. the LangGraph dev server
    /// via `uv run`). Kept configurable so packaging can swap it later.
    pub launch_command: Vec<String>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            port: 2024,
            project_dir: ".".to_string(),
            // TODO(P6.2): confirm the exact dev-server invocation for the
            // deployed graph (langgraph.json defines the graph id).
            launch_command: vec![
                "uv".into(),
                "run".into(),
                "langgraph".into(),
                "dev".into(),
                "--port".into(),
            ],
        }
    }
}

impl BackendConfig {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// Owns the spawned backend process and shuts it down on drop.
pub struct BackendSupervisor {
    config: BackendConfig,
    child: Option<Child>,
}

impl BackendSupervisor {
    pub fn new(config: BackendConfig) -> Self {
        Self { config, child: None }
    }

    /// Spawn the local backend sidecar. Idempotent: a second call is a no-op
    /// while a child is already running.
    pub fn start(&mut self) -> Result<()> {
        if self.child.is_some() {
            return Ok(());
        }
        let mut args = self.config.launch_command.clone();
        // The launch command ends with `--port`; append the actual port.
        args.push(self.config.port.to_string());
        let (program, rest) = args
            .split_first()
            .context("launch_command must not be empty")?;

        tracing::info!(program, port = self.config.port, "spawning backend sidecar");
        let child = Command::new(program)
            .args(rest)
            .current_dir(&self.config.project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn backend: {program}"))?;
        self.child = Some(child);
        Ok(())
    }

    /// Poll the backend's health endpoint until it responds or the budget runs
    /// out. TODO(P6.2): point at the real health/OK route the dev server exposes.
    pub async fn wait_until_healthy(&self, attempts: u32) -> Result<()> {
        let url = format!("{}/ok", self.config.base_url());
        let client = reqwest::Client::new();
        for attempt in 1..=attempts {
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!("backend healthy after {attempt} attempt(s)");
                    return Ok(());
                }
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        anyhow::bail!("backend did not become healthy within {attempts} attempts")
    }
}

impl Drop for BackendSupervisor {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            tracing::info!("terminating backend sidecar");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
