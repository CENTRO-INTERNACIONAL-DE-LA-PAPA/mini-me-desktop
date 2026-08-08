import { memo } from "react";
import type { SubagentRun } from "../types";
import { SubagentCard } from "./SubagentCard";

export const SubagentActivityPanel = memo(function SubagentActivityPanel({
  subagents,
}: {
  subagents: SubagentRun[];
}) {
  if (subagents.length === 0) {
    return (
      <section className="activity-panel activity-panel--empty" aria-label="Subagent activity">
        <p className="sidebar-status">
          <span className="sidebar-status-dot" aria-hidden="true" />
          No subagents running
        </p>
      </section>
    );
  }

  return (
    <section className="activity-panel" aria-label="Subagent activity">
      <div className="panel-heading compact">
        <div>
          <p className="eyebrow">Live workflow</p>
          <h2>Subagents</h2>
        </div>
        <span className="count-badge">{subagents.length}</span>
      </div>

      <div className="subagent-list">
        {subagents.map((subagent) => (
          <SubagentCard key={subagent.id} subagent={subagent} />
        ))}
      </div>
    </section>
  );
});
