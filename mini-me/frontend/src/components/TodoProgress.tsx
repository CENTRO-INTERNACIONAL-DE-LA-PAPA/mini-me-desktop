import { memo } from "react";
import type { TodoItem } from "../types";

function todoLabel(todo: TodoItem) {
  return todo.content ?? todo.title ?? "Untitled task";
}

export const TodoProgress = memo(function TodoProgress({ todos }: { todos: TodoItem[] }) {
  if (!todos.length) return null;

  const completed = todos.filter((todo) => todo.status === "completed").length;
  const pct = Math.round((completed / todos.length) * 100);

  return (
    <section className="todo-panel" aria-label="Run progress">
      <div className="panel-heading compact">
        <div>
          <p className="eyebrow">Plan</p>
          <h2>Progress</h2>
        </div>
        <span className="count-badge">
          {completed}/{todos.length}
        </span>
      </div>

      <div className="progress-track" aria-hidden="true">
        <div className="progress-fill" style={{ width: `${pct}%` }} />
      </div>

      <ol className="todo-list">
        {todos.map((todo, index) => (
          <li key={`${todo.status}-${index}`} className={`todo-item ${todo.status}`}>
            <span className="todo-marker" aria-hidden="true" />
            <span>{todoLabel(todo)}</span>
          </li>
        ))}
      </ol>
    </section>
  );
});
