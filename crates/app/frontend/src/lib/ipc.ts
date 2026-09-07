import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Adopted,
  Answer,
  AttachmentInput,
  DeleteFiles,
  DeleteOutcome,
  FixEvent,
  GalleryListing,
  Job,
  LogPaths,
  PendingAttachment,
  PreflightReport,
  PreparedTurn,
  Project,
  Provider,
  Settings,
  Snapshot,
  Started,
  Subagent,
  TurnEvent,
} from "./protocol";
import type { RawTheme } from "../theme/theme";

export const ipc = {
  getExecutionLabel: () => invoke<string>("get_execution_label"),
  getBaseUrl: () => invoke<string>("get_base_url"),
  getSettings: () => invoke<Settings>("get_settings"),
  getSettingsPath: () => invoke<string>("get_settings_path"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  getSecret: (name: string) => invoke<string | null>("get_secret", { name }),
  setSecretValue: (name: string, value: string) => invoke<void>("set_secret_value", { name, value }),
  getProviders: () => invoke<Provider[]>("get_providers"),
  searchThemes: (query: string) => invoke<GalleryListing[]>("search_themes", { query }),
  installTheme: (id: string) => invoke<string[]>("install_theme", { id }),
  listInstalledThemes: () => invoke<[string, RawTheme][]>("list_installed_themes"),
  listSubagents: () => invoke<Subagent[]>("list_subagents"),
  prepareTurn: (prompt: string, attachments: AttachmentInput[], subagent: string | null) =>
    invoke<PreparedTurn>("prepare_turn", { prompt, attachments, subagent }),
  submitTurn: (prompt: string, attachments: PendingAttachment[]) =>
    invoke<void>("submit_turn", { prompt, attachments }),
  resumeTurn: (answers: Answer[]) => invoke<void>("resume_turn", { answers }),
  cancelTurn: () => invoke<boolean>("cancel_turn"),
  resetThread: () => invoke<void>("reset_thread"),
  getThreadId: () => invoke<string | null>("get_thread_id"),
  setProject: (project: string | null) => invoke<void>("set_project", { project }),
  getProject: () => invoke<string | null>("get_project"),
  fetchProject: () => invoke<Project>("fetch_project"),
  setMission: (mission: string) => invoke<Project>("set_mission", { mission }),
  warmUp: () => invoke<Started | null>("warm_up"),
  warmGraph: () => invoke<void>("warm_graph"),
  restartBackend: () => invoke<Started>("restart_backend"),
  listConversations: (adopt: boolean) => invoke<Adopted>("list_conversations", { adopt }),
  openConversation: (threadId: string) =>
    invoke<[[string, string][], Snapshot | null]>("open_conversation", { threadId }),
  deleteConversations: (threadIds: string[], files: DeleteFiles) =>
    invoke<DeleteOutcome>("delete_conversations", { threadIds, files }),
  renameConversation: (threadId: string, title: string) =>
    invoke<void>("rename_conversation", { threadId, title }),
  sweepFinishedJobs: () => invoke<[string, Job][] | null>("sweep_finished_jobs"),

  runPreflight: () => invoke<PreflightReport>("run_preflight"),
  startFix: (argv: string[]) => invoke<void>("start_fix", { argv }),
  cancelFix: () => invoke<boolean>("cancel_fix"),
  adoptCheckout: (dir: string) => invoke<Settings>("adopt_checkout", { dir }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  getLogPaths: () => invoke<LogPaths>("get_log_paths"),

  onTurnEvent: (callback: (event: TurnEvent) => void): Promise<UnlistenFn> =>
    listen<TurnEvent>("turn-event", (event) => callback(event.payload)),
  onFixEvent: (callback: (event: FixEvent) => void): Promise<UnlistenFn> =>
    listen<FixEvent>("fix-event", (event) => callback(event.payload)),
};
