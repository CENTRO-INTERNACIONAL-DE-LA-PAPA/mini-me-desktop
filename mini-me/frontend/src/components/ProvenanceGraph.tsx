import { memo, useMemo } from "react";
import type { ProvenanceEdge } from "../types";

// Provenance graph (P4): render the investigation as one linked DAG. Nodes are
// the endpoints of the provenance edges (self-describing via kind + label, so an
// endpoint that is not in any artifact slice still renders). Dependency-free SVG
// so there is no chart-library supply-chain surface.

interface ProvenanceGraphProps {
  edges: ProvenanceEdge[];
  // Jump to the tab for a clicked node's artifact kind.
  onFocusKind?: (kind: string) => void;
}

interface GraphNode {
  id: string;
  kind: string;
  label: string;
  rank: number;
}

// Provenance rank = how far downstream an artifact is. Inputs sit on the left,
// derived artifacts flow to the right. Subagents (the producers) sit furthest
// left, since everything else flows out of them.
const RANK: Record<string, number> = {
  subagent: 0,
  source: 1,
  dataset: 1,
  file: 1,
  library: 2,
  hypothesis: 2,
  analysis: 3,
  report: 4,
};

// Edge relations are stored from the derived node's perspective ("hypothesis
// cites paper"); render them as left-to-right flow words along the arrow.
const FLOW_LABEL: Record<string, string> = {
  cites: "supports",
  contradicted_by: "challenges",
  indexes: "indexed in",
  analyzes: "analyzed by",
  produced_by: "produced",
  tests: "tested by",
  synthesizes: "synthesized in",
  derived_from: "feeds",
};

const KIND_LABEL: Record<string, string> = {
  subagent: "Subagent",
  source: "Paper",
  dataset: "Dataset",
  file: "File",
  library: "Library",
  hypothesis: "Theories",
  analysis: "Analysis",
  report: "Report",
};

const PAD = 18;
const NODE_W = 170;
const NODE_H = 48;
const COL_W = 224;
const ROW_H = 70;

function kindOf(id: string, fallback?: string): string {
  if (fallback) return fallback;
  const [prefix] = id.split(":", 1);
  return prefix || "artifact";
}

function labelOf(id: string, kind: string, given?: string): string {
  if (given) return given;
  const value = id.slice(id.indexOf(":") + 1);
  return value || KIND_LABEL[kind] || "Artifact";
}

function truncate(text: string, max = 42): string {
  return text.length <= max ? text : text.slice(0, max - 1) + "…";
}

export const ProvenanceGraph = memo(function ProvenanceGraph({
  edges,
  onFocusKind,
}: ProvenanceGraphProps) {
  const { nodes, positioned, width, height } = useMemo(() => {
    const nodeMap = new Map<string, GraphNode>();
    const add = (id: string, kind?: string, label?: string) => {
      if (nodeMap.has(id)) return;
      const k = kindOf(id, kind);
      nodeMap.set(id, {
        id,
        kind: k,
        label: labelOf(id, k, label),
        rank: RANK[k] ?? 1,
      });
    };
    for (const edge of edges) {
      add(edge.source, edge.sourceKind, edge.sourceLabel);
      add(edge.target, edge.targetKind, edge.targetLabel);
    }

    const allNodes = [...nodeMap.values()];
    // Compact the sparse ranks into sequential columns (no empty gaps).
    const ranks = [...new Set(allNodes.map((n) => n.rank))].sort((a, b) => a - b);
    const colIndex = new Map(ranks.map((r, i) => [r, i]));

    const perColumnCount = new Map<number, number>();
    const pos = new Map<string, { x: number; y: number }>();
    for (const node of allNodes) {
      const col = colIndex.get(node.rank) ?? 0;
      const row = perColumnCount.get(col) ?? 0;
      perColumnCount.set(col, row + 1);
      pos.set(node.id, {
        x: PAD + col * COL_W,
        y: PAD + row * ROW_H,
      });
    }

    const maxRows = Math.max(1, ...perColumnCount.values());
    return {
      nodes: allNodes,
      positioned: pos,
      width: PAD * 2 + (ranks.length - 1) * COL_W + NODE_W,
      height: PAD * 2 + (maxRows - 1) * ROW_H + NODE_H,
    };
  }, [edges]);

  if (nodes.length === 0) return null;

  return (
    <div className="pgraph-scroll">
      <svg
        className="pgraph"
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="Provenance graph: how each artifact links to the inputs it was derived from"
      >
        <defs>
          <marker
            id="pgraph-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="7"
            markerHeight="7"
            orient="auto-start-reverse"
          >
            <path d="M0,0 L10,5 L0,10 z" className="pgraph-arrowhead" />
          </marker>
        </defs>

        {edges.map((edge, i) => {
          // Flow left→right: from the input (target) to the derived node (source).
          const from = positioned.get(edge.target);
          const to = positioned.get(edge.source);
          if (!from || !to) return null;
          const x1 = from.x + NODE_W;
          const y1 = from.y + NODE_H / 2;
          const x2 = to.x;
          const y2 = to.y + NODE_H / 2;
          const dx = Math.max(40, (x2 - x1) / 2);
          const midX = (x1 + x2) / 2;
          const midY = (y1 + y2) / 2;
          const flow = FLOW_LABEL[edge.relation] ?? edge.relation;
          return (
            <g key={`${edge.source}__${edge.target}__${edge.relation}__${i}`} className="pgraph-edge">
              <path
                d={`M ${x1},${y1} C ${x1 + dx},${y1} ${x2 - dx},${y2} ${x2},${y2}`}
                markerEnd="url(#pgraph-arrow)"
              />
              <text x={midX} y={midY - 4} textAnchor="middle" className="pgraph-edge-label">
                {flow}
              </text>
            </g>
          );
        })}

        {nodes.map((node) => {
          const p = positioned.get(node.id);
          if (!p) return null;
          const kindName = KIND_LABEL[node.kind] ?? node.kind;
          return (
            <g
              key={node.id}
              className={`pgraph-node kind-${node.kind}`}
              transform={`translate(${p.x}, ${p.y})`}
              role="button"
              tabIndex={0}
              onClick={() => onFocusKind?.(node.kind)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onFocusKind?.(node.kind);
                }
              }}
            >
              <title>{`${kindName}: ${node.label}`}</title>
              <rect width={NODE_W} height={NODE_H} rx={9} />
              <text x={12} y={19} className="pgraph-node-kind">
                {kindName}
              </text>
              <text x={12} y={36} className="pgraph-node-label">
                {truncate(node.label)}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
});
