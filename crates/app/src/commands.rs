use std::sync::Arc;

use futures::StreamExt;
use tauri::{AppHandle, Emitter, State};

use crate::backend::Started;
use crate::protocol::{Answer, Job, Project};
use crate::sidecar::{Adopted, DeleteFiles, DeleteOutcome, Sidecar};
use crate::workspace::PendingAttachment;

type SidecarState<'r> = State<'r, Arc<Sidecar>>;

#[tauri::command]
pub fn get_execution_label(sidecar: SidecarState<'_>) -> &'static str {
    sidecar.execution()
}

#[tauri::command]
pub fn get_base_url(sidecar: SidecarState<'_>) -> String {
    sidecar.base_url().to_string()
}

#[tauri::command]
pub fn get_settings() -> crate::settings::Settings {
    crate::settings::Settings::load()
}

#[tauri::command]
pub fn save_settings(sidecar: SidecarState<'_>, settings: crate::settings::Settings) -> Result<(), String> {
    settings.save().map_err(|error| format!("{error:#}"))?;
    sidecar.set_model(crate::model_choice(&settings));
    crate::settings::apply_theme(&settings);
    Ok(())
}

#[tauri::command]
pub fn get_secret(name: String) -> Option<String> {
    crate::settings::secret(&name)
}

#[tauri::command]
pub fn set_secret_value(name: String, value: String) -> Result<(), String> {
    crate::settings::set_secret(&name, &value).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn get_providers() -> &'static [crate::settings::Provider] {
    &crate::settings::PROVIDERS
}

#[tauri::command]
pub async fn search_themes(sidecar: SidecarState<'_>, query: String) -> Result<Vec<crate::gallery::Listing>, String> {
    let mut rx = sidecar.search_themes(query);
    rx.next().await.ok_or_else(|| "the sidecar closed".to_string())?
}

#[tauri::command]
pub async fn install_theme(sidecar: SidecarState<'_>, id: String) -> Result<Vec<String>, String> {
    let mut rx = sidecar.install_theme(id);
    rx.next().await.ok_or_else(|| "the sidecar closed".to_string())?
}

#[tauri::command]
pub fn list_installed_themes() -> Vec<(String, crate::theme::Theme)> {
    crate::settings::available_themes()
}

#[tauri::command]
pub fn submit_turn(
    app: AppHandle,
    sidecar: SidecarState<'_>,
    prompt: String,
    attachments: Vec<PendingAttachment>,
) {
    let mut rx = sidecar.submit(prompt, attachments);
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.next().await {
            let _ = app.emit("turn-event", &event);
        }
    });
}

#[tauri::command]
pub fn resume_turn(app: AppHandle, sidecar: SidecarState<'_>, answers: Vec<Answer>) {
    let mut rx = sidecar.resume(answers);
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.next().await {
            let _ = app.emit("turn-event", &event);
        }
    });
}

#[tauri::command]
pub fn cancel_turn(sidecar: SidecarState<'_>) -> bool {
    sidecar.cancel_turn()
}

#[tauri::command]
pub fn reset_thread(sidecar: SidecarState<'_>) {
    sidecar.reset_thread();
}

#[tauri::command]
pub fn get_thread_id(sidecar: SidecarState<'_>) -> Option<String> {
    sidecar.thread_id()
}

#[tauri::command]
pub fn set_project(sidecar: SidecarState<'_>, project: Option<String>) {
    sidecar.set_project(project);
}

#[tauri::command]
pub fn get_project(sidecar: SidecarState<'_>) -> Option<String> {
    sidecar.project()
}

#[tauri::command]
pub async fn fetch_project(sidecar: SidecarState<'_>) -> Result<Project, String> {
    let mut rx = sidecar.fetch_project();
    rx.next().await.ok_or_else(|| "the sidecar closed".to_string())?
}

#[tauri::command]
pub async fn set_mission(sidecar: SidecarState<'_>, mission: String) -> Result<Project, String> {
    let mut rx = sidecar.set_mission(mission);
    rx.next().await.ok_or_else(|| "the sidecar closed".to_string())?
}

#[tauri::command]
pub async fn warm_up(sidecar: SidecarState<'_>) -> Result<Option<Started>, ()> {
    let mut rx = sidecar.warm_up();
    Ok(rx.next().await)
}

#[tauri::command]
pub async fn warm_graph(sidecar: SidecarState<'_>) -> Result<(), String> {
    let mut rx = sidecar.warm_graph();
    match rx.next().await {
        Some(Ok(())) => Ok(()),
        Some(Err(error)) => Err(format!("{error:#}")),
        None => Err("the sidecar closed".to_string()),
    }
}

#[tauri::command]
pub async fn restart_backend(sidecar: SidecarState<'_>) -> Result<Started, String> {
    let mut rx = sidecar.restart_backend();
    match rx.next().await {
        Some(Ok(started)) => Ok(started),
        Some(Err(error)) => Err(format!("{error:#}")),
        None => Err("the sidecar closed".to_string()),
    }
}

#[tauri::command]
pub async fn list_conversations(sidecar: SidecarState<'_>, adopt: bool) -> Result<Adopted, ()> {
    let mut rx = sidecar.list_conversations(adopt);
    Ok(rx.next().await.unwrap_or(Adopted {
        conversations: Vec::new(),
        scanned: false,
    }))
}

#[tauri::command]
pub async fn open_conversation(
    sidecar: SidecarState<'_>,
    thread_id: String,
) -> Result<(Vec<(String, String)>, Option<crate::protocol::Snapshot>), ()> {
    let mut rx = sidecar.open_conversation(thread_id);
    Ok(rx.next().await.unwrap_or_default())
}

#[tauri::command]
pub async fn delete_conversations(
    sidecar: SidecarState<'_>,
    thread_ids: Vec<String>,
    files: DeleteFiles,
) -> Result<DeleteOutcome, String> {
    let mut rx = sidecar.delete_conversations(thread_ids, files);
    match rx.next().await {
        Some(Ok(outcome)) => Ok(outcome),
        Some(Err(error)) => Err(format!("{error:#}")),
        None => Err("the sidecar closed".to_string()),
    }
}

#[tauri::command]
pub fn rename_conversation(sidecar: SidecarState<'_>, thread_id: String, title: String) {
    sidecar.rename_conversation(thread_id, title);
}

#[tauri::command]
pub async fn sweep_finished_jobs(sidecar: SidecarState<'_>) -> Result<Option<Vec<(String, Job)>>, ()> {
    let mut rx = sidecar.sweep_finished_jobs();
    Ok(rx.next().await.flatten())
}
