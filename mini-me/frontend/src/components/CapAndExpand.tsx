import { Fragment, useState, type ReactNode } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";

interface CapAndExpandProps<T> {
  items: ReadonlyArray<T>;
  cap?: number;
  noun: string;
  keyOf: (item: T) => string;
  renderItem: (item: T, index: number) => ReactNode;
}

export function CapAndExpand<T>({
  items,
  cap = 5,
  noun,
  keyOf,
  renderItem,
}: CapAndExpandProps<T>) {
  const [expanded, setExpanded] = useState(false);
  const total = items.length;
  const overflow = Math.max(0, total - cap);
  const visible = expanded || overflow === 0 ? items : items.slice(0, cap);

  return (
    <>
      {visible.map((item, index) => (
        <Fragment key={keyOf(item)}>{renderItem(item, index)}</Fragment>
      ))}
      {overflow > 0 ? (
        <button
          className="cap-expand-toggle"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded ? (
            <>
              <ChevronUp size={14} aria-hidden="true" />
              Show fewer
            </>
          ) : (
            <>
              <ChevronDown size={14} aria-hidden="true" />
              Show all ({total} {noun})
            </>
          )}
        </button>
      ) : null}
    </>
  );
}
