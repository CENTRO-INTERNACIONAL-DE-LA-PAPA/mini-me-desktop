import { create } from "zustand";
import { ipc } from "./ipc";
import { notifyTurnFinished } from "./notify";
import type { Answer, ApprovalRequest, AsyncTask, Conversation, Job, Project, Snapshot, TurnEvent } from "./protocol";

export interface TraceStep {
  agent: string | null;
  label: string;
}

export interface Message {
  role: "user" | "assistant";
  text: string;
  steps?: TraceStep[];
  subagentText?: Record<string, string>;
}

export type SidebarView = "conversations" | "projects";

interface AppState {
  status: string;
  error: string | null;
  streaming: boolean;
  executionLabel: string;
  baseUrl: string;
  approveConversation: boolean;
  transcript: Message[];
  snapshot: Snapshot | null;
  pendingApproval: ApprovalRequest | null;
  jobs: Job[];
  tasks: AsyncTask[];

  conversations: Conversation[];
  conversationsLoaded: boolean;
  currentThreadId: string | null;
  sidebarView: SidebarView;
  sidebarOpen: boolean;

  setExecutionInfo: (executionLabel: string, baseUrl: string) => void;
  setApproveConversation: (value: boolean) => void;
  setSnapshotProject: (project: Project) => void;
  beginTurn: (prompt: string) => void;
  submitTurn: (prompt: string, attachments?: { source: string; reference: string }[]) => void;
  applyTurnEvent: (event: TurnEvent) => void;
  answerApproval: (answers: Answer[]) => void;
  cancelTurn: () => void;

  setSidebarView: (view: SidebarView) => void;
  toggleSidebar: () => void;
  loadConversations: () => Promise<void>;
  openConversation: (threadId: string) => Promise<void>;
  startNewConversation: (project: string | null) => void;
  renameConversation: (threadId: string, title: string) => Promise<void>;
  deleteConversation: (conversation: Conversation) => Promise<void>;
}

export const useAppStore = create<AppState>((set, get) => ({
  status: "idle",
  error: null,
  streaming: false,
  executionLabel: "",
  baseUrl: "",
  approveConversation: false,
  transcript: [],
  snapshot: null,
  pendingApproval: null,
  jobs: [],
  tasks: [],

  conversations: [],
  conversationsLoaded: false,
  currentThreadId: null,
  sidebarView: "conversations",
  sidebarOpen: true,

  setExecutionInfo: (executionLabel, baseUrl) => set({ executionLabel, baseUrl }),
  setApproveConversation: (value) => set({ approveConversation: value }),
  setSnapshotProject: (project) =>
    set((state) => ({
      snapshot: state.snapshot
        ? { ...state.snapshot, project }
        : {
            buckets: [],
            project,
            jobs: [],
            drafts: [],
            tasks: [],
            reports: [],
            todos: [],
            datasets: [],
            documents: [],
            sources: [],
          },
    })),
  setSidebarView: (view) => set({ sidebarView: view }),
  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),

  loadConversations: async () => {
    const adopted = await ipc.listConversations(true);
    set({ conversations: adopted.conversations, conversationsLoaded: true });
  },

  openConversation: async (threadId) => {
    const [transcriptPairs, snapshot] = await ipc.openConversation(threadId);
    set({
      currentThreadId: threadId,
      snapshot,
      transcript: transcriptPairs.map(([role, text]) => ({
        role: role === "user" ? "user" : "assistant",
        text,
      })),
      status: "idle",
      error: null,
    });
  },

  startNewConversation: (project) => {
    ipc.resetThread();
    ipc.setProject(project);
    set({ currentThreadId: null, transcript: [], snapshot: null, status: "idle", error: null });
  },

  renameConversation: async (threadId, title) => {
    await ipc.renameConversation(threadId, title);
    set((state) => ({
      conversations: state.conversations.map((c) => (c.thread_id === threadId ? { ...c, title } : c)),
    }));
  },

  deleteConversation: async (conversation) => {
    await ipc.deleteConversations([conversation.thread_id], {
      Conversation: { project: conversation.project, thread_id: conversation.thread_id },
    });
    set((state) => ({
      conversations: state.conversations.filter((c) => c.thread_id !== conversation.thread_id),
      currentThreadId: state.currentThreadId === conversation.thread_id ? null : state.currentThreadId,
    }));
    if (get().currentThreadId === null) {
      get().startNewConversation(null);
    }
  },

  beginTurn: (prompt) =>
    set((state) => ({
      transcript: [...state.transcript, { role: "user", text: prompt }, { role: "assistant", text: "" }],
      streaming: true,
      error: null,
      status: "thinking",
    })),

  submitTurn: (prompt, attachments = []) => {
    get().beginTurn(prompt);
    ipc.submitTurn(prompt, attachments);
  },

  cancelTurn: () => {
    ipc.cancelTurn();
    set({ streaming: false, status: "idle" });
  },

  applyTurnEvent: (event) => {
    if (event.type === "Done") {
      const last = get().transcript[get().transcript.length - 1];
      notifyTurnFinished(last?.text ? last.text.slice(0, 200) : "Your turn finished.");
    } else if (event.type === "Error") {
      notifyTurnFinished(`Something went wrong: ${event.data}`);
    }
    set((state) => {
      switch (event.type) {
        case "Status":
          return { status: event.data };
        case "Token": {
          const transcript = [...state.transcript];
          const last = transcript[transcript.length - 1];
          if (last?.role === "assistant") {
            transcript[transcript.length - 1] = { ...last, text: last.text + event.data };
          }
          return { transcript };
        }
        case "Step": {
          const transcript = [...state.transcript];
          const last = transcript[transcript.length - 1];
          if (last?.role === "assistant") {
            const steps = [...(last.steps ?? []), { agent: event.data.agent?.name ?? null, label: event.data.label }];
            transcript[transcript.length - 1] = { ...last, steps };
          }
          return { transcript };
        }
        case "SubagentToken": {
          const transcript = [...state.transcript];
          const last = transcript[transcript.length - 1];
          if (last?.role === "assistant") {
            const key = event.data.agent.name;
            const subagentText = { ...(last.subagentText ?? {}) };
            subagentText[key] = (subagentText[key] ?? "") + event.data.text;
            transcript[transcript.length - 1] = { ...last, subagentText };
          }
          return { transcript };
        }
        case "Snapshot": {
          const incoming = event.data;
          const project =
            incoming.project && incoming.project.suggestions.length === 0 && state.snapshot?.project
              ? { ...incoming.project, suggestions: state.snapshot.project.suggestions }
              : incoming.project;
          return {
            snapshot: { ...incoming, project },
            jobs: incoming.jobs,
            tasks: incoming.tasks,
          };
        }
        case "Approval":
          return { pendingApproval: event.data, streaming: false };
        case "Done":
          return { streaming: false, status: "idle" };
        case "Error":
          return { streaming: false, status: "idle", error: event.data };
        default:
          return {};
      }
    });
  },

  answerApproval: (answers) => {
    ipc.resumeTurn(answers);
    set({ pendingApproval: null, streaming: true });
  },
}));
