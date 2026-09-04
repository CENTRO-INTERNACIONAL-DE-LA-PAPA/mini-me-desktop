import { useEffect, useMemo, useState } from "react";
import { Actions, Button, Icon, Label, Menu, MenuItem, Modal, SearchBar } from "../components";
import { useAppStore } from "../lib/store";
import type { Conversation } from "../lib/protocol";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

function matchScore(query: string, title: string): number | null {
  if (query.trim() === "") return 0;
  const q = query.toLowerCase();
  const t = title.toLowerCase();
  return t.includes(q) ? q.length : null;
}

type RowMenu = { kind: "conversation"; conversation: Conversation } | { kind: "project"; name: string };

export function Sidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { theme } = useTheme();
  const {
    conversations,
    conversationsLoaded,
    currentThreadId,
    sidebarView,
    setSidebarView,
    loadConversations,
    openConversation,
    startNewConversation,
    renameConversation,
    deleteConversation,
    toggleSidebar,
  } = useAppStore((state) => ({
    conversations: state.conversations,
    conversationsLoaded: state.conversationsLoaded,
    currentThreadId: state.currentThreadId,
    sidebarView: state.sidebarView,
    setSidebarView: state.setSidebarView,
    loadConversations: state.loadConversations,
    openConversation: state.openConversation,
    startNewConversation: state.startNewConversation,
    renameConversation: state.renameConversation,
    deleteConversation: state.deleteConversation,
    toggleSidebar: state.toggleSidebar,
  }));

  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; target: RowMenu } | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Conversation | null>(null);
  const [renameValue, setRenameValue] = useState("");

  useEffect(() => {
    loadConversations();
  }, [loadConversations]);

  const matched = useMemo(
    () => conversations.filter((c) => matchScore(query, c.title) !== null),
    [conversations, query],
  );

  const openMenu = (e: React.MouseEvent, target: RowMenu) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ at: { x: e.clientX, y: e.clientY + 6 }, target });
  };

  const startRename = (conversation: Conversation) => {
    setRenaming(conversation.thread_id);
    setRenameValue(conversation.title);
    setMenu(null);
  };

  const commitRename = (threadId: string) => {
    if (renameValue.trim()) renameConversation(threadId, renameValue.trim());
    setRenaming(null);
  };

  const row = (conversation: Conversation) => {
    const selected = currentThreadId === conversation.thread_id;
    if (renaming === conversation.thread_id) {
      return (
        <input
          key={conversation.thread_id}
          autoFocus
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onBlur={() => commitRename(conversation.thread_id)}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitRename(conversation.thread_id);
            if (e.key === "Escape") setRenaming(null);
          }}
          style={{
            width: "100%",
            padding: "6px 10px",
            borderRadius: 6,
            border: `1px solid ${hex(theme.accent)}`,
            background: hex(theme.surface),
            color: hex(theme.text),
            fontSize: 13,
          }}
        />
      );
    }
    return (
      <div
        key={conversation.thread_id}
        onClick={() => openConversation(conversation.thread_id)}
        onContextMenu={(e) => openMenu(e, { kind: "conversation", conversation })}
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "center",
          gap: 4,
          width: "100%",
          minWidth: 0,
          padding: "4px 8px",
          borderRadius: 6,
          background: hex(selected ? theme.accentSoft : theme.surface),
          color: hex(selected ? theme.accent : theme.textMuted),
          fontSize: 13,
          cursor: "pointer",
        }}
      >
        <div style={{ minWidth: 0, flexGrow: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {conversation.title}
        </div>
        <div
          onClick={(e) => openMenu(e, { kind: "conversation", conversation })}
          style={{ flex: "none", color: hex(theme.textFaint), padding: "0 4px", cursor: "pointer" }}
        >
          ⋮
        </div>
      </div>
    );
  };

  const grouped = useMemo(() => {
    const map = new Map<string, Conversation[]>();
    for (const c of matched) {
      if (c.project) {
        if (!map.has(c.project)) map.set(c.project, []);
        map.get(c.project)!.push(c);
      }
    }
    return map;
  }, [matched]);

  const ungrouped = matched.filter((c) => !c.project);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: 260,
        height: "100%",
        flex: "none",
        margin: 8,
        padding: 12,
        gap: 16,
        borderRadius: 10,
        background: hex(theme.surface),
        border: `1px solid ${hex(theme.border)}`,
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
        <div
          onClick={() => startNewConversation(null)}
          style={{ fontSize: 16, fontWeight: 500, cursor: "pointer", color: hex(theme.text) }}
        >
          Mini-Me App
        </div>
        <Button icon="icons/sidebar-simple-left.svg" style="secondaryWhite" border={false} onClick={toggleSidebar} />
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div
          style={{
            display: "flex",
            flexDirection: "row",
            borderRadius: 6,
            border: `1px solid ${hex(theme.border)}`,
            background: hex(theme.background),
          }}
        >
          {(["conversations", "projects"] as const).map((view) => (
            <div
              key={view}
              onClick={() => setSidebarView(view)}
              style={{
                flexGrow: 1,
                textAlign: "center",
                padding: "6px 0",
                fontSize: 13,
                borderRadius: 6,
                cursor: "pointer",
                color: hex(sidebarView === view ? theme.accent : theme.textMuted),
                background: sidebarView === view ? hex(theme.accentSoft) : "transparent",
              }}
            >
              {view === "conversations" ? "Conversations" : "Projects"}
            </div>
          ))}
        </div>
        <SearchBar value={query} placeholder="Search conversations…" onChange={setQuery} />
      </div>

      <div style={{ display: "flex", flexDirection: "column", flexGrow: 1, minHeight: 0, overflowY: "auto", gap: 4 }} className="thin-scroll">
        {sidebarView === "conversations" ? (
          ungrouped.length === 0 ? (
            <div style={{ padding: 8, fontSize: 12, color: hex(theme.textFaint) }}>
              {!conversationsLoaded
                ? "Loading your conversations…"
                : conversations.length === 0
                  ? "Conversations you start will appear here."
                  : "Nothing matches that."}
            </div>
          ) : (
            ungrouped.map(row)
          )
        ) : grouped.size === 0 ? (
          <div style={{ padding: 8, fontSize: 12, color: hex(theme.textFaint) }}>
            {!conversationsLoaded ? "Loading your conversations…" : "Projects you create will appear here."}
          </div>
        ) : (
          Array.from(grouped.entries()).map(([project, items]) => (
            <div key={project} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <div
                onContextMenu={(e) => openMenu(e, { kind: "project", name: project })}
                style={{
                  display: "flex",
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 8,
                  padding: "8px 8px 4px 8px",
                }}
              >
                <Icon path="icons/folder.svg" size="small" colour={theme.textMuted} />
                <div style={{ fontSize: 13, color: hex(theme.textMuted) }}>{project}</div>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 4, marginLeft: 16, paddingLeft: 8, borderLeft: `1px solid ${hex(theme.border)}` }}>
                {items.map(row)}
              </div>
            </div>
          ))
        )}

        <Button
          icon="icons/plus.svg"
          onClick={() => (sidebarView === "conversations" ? startNewConversation(null) : undefined)}
        >
          {sidebarView === "conversations" ? "New Conversation" : "New Project"}
        </Button>
      </div>

      <Button icon="icons/gear-six.svg" onClick={onOpenSettings}>
        Settings
      </Button>

      {menu && menu.target.kind === "conversation" && (
        <Menu at={menu.at} onDismiss={() => setMenu(null)}>
          <MenuItem label="Rename" onClick={() => startRename((menu.target as { kind: "conversation"; conversation: Conversation }).conversation)} />
          <MenuItem
            label="Delete"
            danger
            onClick={() => {
              setConfirmDelete((menu.target as { kind: "conversation"; conversation: Conversation }).conversation);
              setMenu(null);
            }}
          />
        </Menu>
      )}

      {confirmDelete && (
        <Modal
          title="Delete conversation?"
          width={480}
          onDismiss={() => setConfirmDelete(null)}
          body={
            <>
              <Label>{`This permanently deletes "${confirmDelete.title}", its chat history, and every saved file it produced.`}</Label>
            </>
          }
          actions={
            <Actions>
              <div style={{ flexGrow: 1 }} />
              <Button onClick={() => setConfirmDelete(null)}>Cancel</Button>
              <Button
                style="danger"
                onClick={() => {
                  deleteConversation(confirmDelete);
                  setConfirmDelete(null);
                }}
              >
                Delete conversation
              </Button>
            </Actions>
          }
          footer={
            <Label size="compact" colour={theme.error}>
              There is no undo.
            </Label>
          }
        />
      )}
    </div>
  );
}
