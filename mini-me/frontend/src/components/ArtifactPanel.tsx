import { Archive, BookMarked, ChevronDown, ChevronRight, Database, FileCode2, FileText, FlaskConical, Image as ImageIcon, Lightbulb, LibraryBig, Loader2, Share2 } from "lucide-react";
import { memo, useEffect, useMemo, useState } from "react";
import type {
  DataAnalysisArtifact,
  DatasetArtifact,
  FileArtifact,
  HypothesisArtifact,
  LibraryArtifact,
  ProjectArtifact,
  ProvenanceEdge,
  ReportArtifact,
  SourceArtifact,
} from "../types";
import { AnalysisFindingCard } from "./AnalysisCard";
import { CapAndExpand } from "./CapAndExpand";
import { DatasetCard } from "./DatasetCard";
import { FileCard } from "./FileCard";
import { ImageGrid } from "./ImageGrid";
import { LibraryCard, LibrarySearchBox } from "./LibraryCard";
import { MarkdownReport } from "./MarkdownReport";
import { ProjectPanel } from "./ProjectPanel";
import { ProvenanceGraph } from "./ProvenanceGraph";
import { SourceCard } from "./SourceCard";
import { TheoryCard } from "./TheoryCard";
import { nodeId, paperNodeId } from "../lib/artifacts";
import { useTheorizerStatus } from "../lib/theorizerClient";
import { useDataVoyagerStatus } from "../lib/dataVoyagerClient";
import type { ProjectControls, ProjectEdit } from "../lib/projectClient";

// Deterministic edges are self-describing: an endpoint may be a paper/subagent
// node that is not in any artifact slice, and we still render it from the edge's
// label. Declared (LLM-asserted) edges must instead resolve to a REAL artifact
// node, or they are dropped at render — so a paraphrased/invented ref never
// shows a fabricated link.
const DETERMINISTIC_RELATIONS = new Set([
  "cites",
  "contradicted_by",
  "indexes",
  "analyzes",
  "produced_by",
]);

interface ArtifactPanelProps {
  threadId: string | null;
  datasets: DatasetArtifact[];
  sources: SourceArtifact[];
  files: FileArtifact[];
  report?: ReportArtifact | null;
  hypothesis?: HypothesisArtifact | null;
  library?: LibraryArtifact | null;
  analysis?: DataAnalysisArtifact | null;
  project?: ProjectArtifact | null;
  edges?: ProvenanceEdge[];
  onRenderReport?: (report: ReportArtifact) => Promise<void>;
  onSearchLibrary?: (query: string) => void;
  onPromoteSuggestion?: (prompt: string) => void;
  onEditProject?: (edit: ProjectEdit) => void;
  projectControls?: ProjectControls;
  collapsed?: boolean;
  width?: number;
}

const SECTION_CAP = 5;
const IMAGE_CAP = 9;

type TabKey = "graph" | "datasets" | "sources" | "theories" | "library" | "analysis" | "images" | "files" | "report";

// Map a provenance-graph node's artifact kind to the tab that shows it, so
// clicking a node jumps to its artifact.
const KIND_TO_TAB: Record<string, TabKey> = {
  dataset: "datasets",
  source: "sources",
  hypothesis: "theories",
  library: "library",
  analysis: "analysis",
  file: "files",
  report: "report",
};

// Collapsible "Knowledge gaps" block under the theories. Defaults collapsed so
// the theories stay the headline; the gaps are secondary context on demand.
function KnowledgeGaps({ gaps }: { gaps: string[] }) {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <div className="knowledge-gaps">
      <button
        type="button"
        className="knowledge-gaps-toggle"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
      >
        {isOpen ? (
          <ChevronDown size={15} aria-hidden="true" />
        ) : (
          <ChevronRight size={15} aria-hidden="true" />
        )}
        <span>Knowledge gaps</span>
        <span className="ct">{gaps.length}</span>
      </button>
      {isOpen ? (
        <ul>
          {gaps.map((gap, gapIndex) => (
            <li key={gapIndex}>{gap}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

// Memoized: artifact arrays are reference-stable in App, so this only
// re-renders when an artifact actually changes (not on every streamed token).
export const ArtifactPanel = memo(function ArtifactPanel({
  threadId,
  datasets,
  sources,
  files,
  report,
  hypothesis,
  library,
  analysis,
  project,
  edges = [],
  onRenderReport,
  onSearchLibrary,
  onPromoteSuggestion,
  onEditProject,
  projectControls,
  collapsed = false,
  width,
}: ArtifactPanelProps) {
  const images = files.filter((f) => f.mediaType?.startsWith("image/"));
  const otherFiles = files.filter((f) => !f.mediaType?.startsWith("image/"));
  // While a run is still generating, poll its status so the card fills in on
  // its own; `liveHypothesis` is the resolved artifact once it completes.
  const { display: liveHypothesis, elapsedSeconds } = useTheorizerStatus(
    threadId,
    hypothesis ?? null,
  );
  const theoriesRunning = liveHypothesis?.status === "running";
  const theories = liveHypothesis?.theories ?? [];
  const libraryPapers = library?.papers ?? [];
  // Same live-poll pattern for DataVoyager: while a run is generating, poll the
  // analyze-data status route so the Analysis card fills in on its own.
  const { display: liveAnalysis } = useDataVoyagerStatus(threadId, analysis ?? null);
  const analysisRunning = liveAnalysis?.status === "running";
  const analysisFailed = liveAnalysis?.status === "failed";
  const analysisFindings = liveAnalysis?.findings ?? [];

  // Validate the provenance graph: keep deterministic (self-describing) edges as
  // is; keep declared edges only when their target resolves to a real artifact
  // node currently in state. `realNodeIds` mirrors the backend node-id convention.
  const graphEdges = useMemo(() => {
    const real = new Set<string>();
    datasets.forEach((d) =>
      real.add(nodeId("dataset", { persistentId: d.persistentId, title: d.title })),
    );
    sources.forEach((s) => real.add(nodeId("source", { link: s.link, citation: s.citation })));
    files.forEach((f) => real.add(nodeId("file", { path: f.path })));
    if (report) real.add(nodeId("report", { title: report.title }));
    const hyp = liveHypothesis ?? hypothesis ?? null;
    if (hyp) {
      real.add(nodeId("hypothesis", { question: hyp.question }));
      hyp.theories.forEach((t) =>
        [...t.supportingPapers, ...t.conflictingPapers].forEach((p) =>
          real.add(
            paperNodeId({ url: p.url, doi: p.doi, corpusId: p.corpusId, citation: p.citation }),
          ),
        ),
      );
    }
    if (library) {
      real.add(nodeId("library", { indexPath: library.indexPath }));
      library.papers.forEach((p) => real.add(paperNodeId({ doi: p.doi, title: p.title })));
    }
    const ana = liveAnalysis ?? analysis ?? null;
    if (ana) real.add(nodeId("analysis", { question: ana.question }));

    return edges.filter(
      (e) => DETERMINISTIC_RELATIONS.has(e.relation) || real.has(e.target),
    );
  }, [edges, datasets, sources, files, report, hypothesis, liveHypothesis, library, analysis, liveAnalysis]);

  const tabs = (
    [
      { key: "graph", label: "Graph", count: graphEdges.length },
      { key: "datasets", label: "Datasets", count: datasets.length },
      { key: "sources", label: "Sources", count: sources.length },
      { key: "theories", label: "Theories", count: theories.length },
      { key: "library", label: "Library", count: library ? library.paperCount || libraryPapers.length : 0 },
      { key: "analysis", label: "Analysis", count: analysisFindings.length },
      { key: "images", label: "Images", count: images.length },
      { key: "files", label: "Files", count: otherFiles.length },
      { key: "report", label: "Report", count: report ? 1 : 0 },
    ] satisfies { key: TabKey; label: string; count: number }[]
    // Keep the Theories / Analysis tabs visible whenever their run produced an
    // artifact — even with zero findings — so the question, status, and any
    // narrative are still surfaced instead of silently disappearing (which reads
    // as "the run produced nothing").
  ).filter(
    (tab) =>
      tab.count > 0 ||
      (tab.key === "theories" && !!liveHypothesis) ||
      (tab.key === "analysis" && !!liveAnalysis),
  );

  // Jump from a clicked graph node to the tab that shows its artifact.
  const focusKind = (kind: string) => {
    const tab = KIND_TO_TAB[kind];
    if (tab && tabs.some((t) => t.key === tab)) setActiveTab(tab);
  };

  const [activeTab, setActiveTab] = useState<TabKey | null>(tabs[0]?.key ?? null);

  // Keep the active tab valid as artifacts stream in or the thread changes:
  // fall back to the first available tab whenever the current one empties out.
  const availableKeys = tabs.map((tab) => tab.key).join(",");
  useEffect(() => {
    const keys = availableKeys ? (availableKeys.split(",") as TabKey[]) : [];
    if (keys.length === 0) {
      if (activeTab !== null) setActiveTab(null);
    } else if (!activeTab || !keys.includes(activeTab)) {
      setActiveTab(keys[0]);
    }
  }, [availableKeys, activeTab]);

  return (
    <aside
      className={`artifact-panel${collapsed ? " collapsed" : ""}`}
      aria-label="Research artifacts"
      aria-hidden={collapsed}
      style={!collapsed && width ? { width } : undefined}
    >
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Outputs</p>
          <h2>Artifacts</h2>
        </div>
      </div>

      {/* Persistent research-project spine: shows above the tabs so the
          mission and suggested next steps are visible even before (or without)
          any other artifacts for this thread. When explicit Projects are wired
          (projectControls), always render so the project switcher + Autopilot
          toggle are visible even before any spine content exists — falling back
          to an empty spine. */}
      {project || projectControls ? (
        <ProjectPanel
          project={project ?? { mission: "", completed: [], pending: [], suggestions: [], plan: null }}
          onPromote={onPromoteSuggestion}
          onEdit={onEditProject}
          controls={projectControls}
        />
      ) : null}

      {tabs.length > 0 ? (
        <>
          <div className="artifact-tabs" role="tablist" aria-label="Artifact categories">
            {tabs.map((tab) => (
              <button
                key={tab.key}
                type="button"
                role="tab"
                aria-selected={activeTab === tab.key}
                className={`artifact-tab${activeTab === tab.key ? " active" : ""}`}
                onClick={() => setActiveTab(tab.key)}
              >
                {tab.label}
                <span className="ct">{tab.count}</span>
              </button>
            ))}
          </div>

          {activeTab === "graph" ? (
            <section className="artifact-section">
              <div className="section-title">
                <Share2 size={16} aria-hidden="true" />
                <h3>Provenance graph</h3>
              </div>
              <p className="library-hint">
                How each artifact links to the inputs it was derived from. Click a
                node to open it.
              </p>
              <ProvenanceGraph edges={graphEdges} onFocusKind={focusKind} />
            </section>
          ) : null}

          {activeTab === "datasets" ? (
            <section className="artifact-section">
              <div className="section-title">
                <Database size={16} aria-hidden="true" />
                <h3>Datasets</h3>
              </div>
              <CapAndExpand
                items={datasets}
                cap={SECTION_CAP}
                noun="datasets"
                keyOf={(dataset) => dataset.persistentId}
                renderItem={(dataset) => <DatasetCard dataset={dataset} />}
              />
            </section>
          ) : null}

          {activeTab === "sources" ? (
            <section className="artifact-section">
              <div className="section-title">
                <LibraryBig size={16} aria-hidden="true" />
                <h3>Sources</h3>
              </div>
              <CapAndExpand
                items={sources}
                cap={SECTION_CAP}
                noun="sources"
                keyOf={(source) => source.citation}
                renderItem={(source) => <SourceCard source={source} />}
              />
            </section>
          ) : null}

          {activeTab === "theories" && liveHypothesis ? (
            <section className="artifact-section">
              <div className="section-title">
                <Lightbulb size={16} aria-hidden="true" />
                <h3>Theories</h3>
                {theoriesRunning ? (
                  <span className="ct theory-running">
                    <Loader2 size={12} aria-hidden="true" className="spin" />
                    generating
                  </span>
                ) : (
                  <span className="ct">{liveHypothesis.papersReviewed} papers reviewed</span>
                )}
              </div>
              {liveHypothesis.question ? (
                <p className="theory-question">{liveHypothesis.question}</p>
              ) : null}
              {theoriesRunning ? (
                <div className="theory-progress">
                  <Loader2 size={16} aria-hidden="true" className="spin" />
                  <p>
                    Generating theories — searching the literature and synthesizing
                    candidate theories
                    {typeof elapsedSeconds === "number"
                      ? ` (~${Math.floor(elapsedSeconds / 60)} min elapsed)`
                      : ""}
                    . This usually takes 5–15 minutes; they'll appear here
                    automatically.
                  </p>
                </div>
              ) : theories.length > 0 ? (
                <CapAndExpand
                  items={theories}
                  cap={SECTION_CAP}
                  noun="theories"
                  keyOf={(theory) =>
                    theory.laws.join("§") ||
                    theory.supportingPapers.map((p) => p.citation).join("§")
                  }
                  renderItem={(theory, index) => <TheoryCard theory={theory} index={index} />}
                />
              ) : (
                <p className="library-hint">
                  The theorizer run finished but returned no structured theories
                  {liveHypothesis.papersReviewed > 0
                    ? ` (reviewed ${liveHypothesis.papersReviewed} papers)`
                    : ""}
                  . See the knowledge gaps below, or try a more specific research
                  question.
                </p>
              )}
              {liveHypothesis.knowledgeGaps.length > 0 ? (
                <KnowledgeGaps gaps={liveHypothesis.knowledgeGaps} />
              ) : null}
            </section>
          ) : null}

          {activeTab === "library" && library ? (
            <section className="artifact-section">
              <div className="section-title">
                <BookMarked size={16} aria-hidden="true" />
                <h3>Library</h3>
                <span className="ct">{library.paperCount} indexed</span>
              </div>
              {library.summary ? (
                <p className="library-summary">{library.summary}</p>
              ) : null}
              {onSearchLibrary ? (
                <LibrarySearchBox onSearch={onSearchLibrary} />
              ) : null}
              {libraryPapers.length > 0 ? (
                <CapAndExpand
                  items={libraryPapers}
                  cap={SECTION_CAP}
                  noun="documents"
                  keyOf={(paper) => paper.path || paper.title}
                  renderItem={(paper) => <LibraryCard paper={paper} />}
                />
              ) : (
                <p className="library-hint">
                  {library.queryHint || "Upload PDFs and ask me to index them."}
                </p>
              )}
            </section>
          ) : null}

          {activeTab === "analysis" && liveAnalysis ? (
            <section className="artifact-section">
              <div className="section-title">
                <FlaskConical size={16} aria-hidden="true" />
                <h3>Analysis</h3>
                {analysisRunning ? (
                  <span className="ct theory-running">
                    <Loader2 size={12} aria-hidden="true" className="spin" />
                    running
                  </span>
                ) : (
                  <span className="ct">DataVoyager</span>
                )}
              </div>
              {liveAnalysis.question ? (
                <p className="theory-question">{liveAnalysis.question}</p>
              ) : null}
              {analysisRunning ? (
                <div className="theory-progress">
                  <Loader2 size={16} aria-hidden="true" className="spin" />
                  <p>
                    DataVoyager is analyzing your data — writing and running code to
                    test hypotheses against it. This usually takes a few minutes to
                    tens of minutes; the results will appear here automatically.
                  </p>
                </div>
              ) : (
                <>
                  {liveAnalysis.summary ? (
                    <p className="analysis-summary">{liveAnalysis.summary}</p>
                  ) : null}
                  {analysisFailed && !liveAnalysis.summary ? (
                    <p className="library-hint">
                      The DataVoyager run did not complete. Try again, or refine the
                      question.
                    </p>
                  ) : null}
                  {liveAnalysis.hypothesesTested.length > 0 ? (
                    <div className="analysis-hypotheses">
                      <p className="analysis-subhead">Hypotheses tested</p>
                      <ul>
                        {liveAnalysis.hypothesesTested.map((hyp, hypIndex) => (
                          <li key={hypIndex}>{hyp}</li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                  {analysisFindings.length > 0 ? (
                    <CapAndExpand
                      items={analysisFindings}
                      cap={SECTION_CAP}
                      noun="findings"
                      keyOf={(finding) => finding.title}
                      renderItem={(finding) => <AnalysisFindingCard finding={finding} />}
                    />
                  ) : !analysisFailed && liveAnalysis.summary ? (
                    <p className="library-hint">
                      Charts and the notebook are in the Images and Files tabs. Ask
                      me to summarize the findings for a structured breakdown.
                    </p>
                  ) : null}
                  {liveAnalysis.charts.length > 0 ? (
                    <p className="analysis-charts-note">
                      {liveAnalysis.charts.length} chart
                      {liveAnalysis.charts.length === 1 ? "" : "s"} produced — see the
                      Images tab.
                    </p>
                  ) : null}
                </>
              )}
            </section>
          ) : null}

          {activeTab === "images" ? (
            <section className="artifact-section">
              <div className="section-title">
                <ImageIcon size={16} aria-hidden="true" />
                <h3>Images</h3>
              </div>
              <ImageGrid images={images} threadId={threadId} cap={IMAGE_CAP} />
            </section>
          ) : null}

          {activeTab === "files" ? (
            <section className="artifact-section">
              <div className="section-title">
                <FileCode2 size={16} aria-hidden="true" />
                <h3>Files</h3>
              </div>
              <CapAndExpand
                items={otherFiles}
                cap={SECTION_CAP}
                noun="files"
                keyOf={(file) => file.path}
                renderItem={(file) => <FileCard file={file} threadId={threadId} />}
              />
            </section>
          ) : null}

          {activeTab === "report" && report ? (
            <section className="artifact-section">
              <div className="section-title">
                <FileText size={16} aria-hidden="true" />
                <h3>Report</h3>
              </div>
              <MarkdownReport report={report} onRender={onRenderReport} />
            </section>
          ) : null}
        </>
      ) : (
        <>
          <div className="artifact-empty">
            <Archive size={20} aria-hidden="true" />
            <p>No artifacts for this thread yet</p>
            <span>they appear as the agent works</span>
          </div>
          <div className="artifact-hint">
            <div className="artifact-hint-row">
              <Database size={14} aria-hidden="true" />
              <span>Datasets pulled from catalogs</span>
            </div>
            <div className="artifact-hint-row">
              <LibraryBig size={14} aria-hidden="true" />
              <span>Literature sources with citations</span>
            </div>
            <div className="artifact-hint-row">
              <ImageIcon size={14} aria-hidden="true" />
              <span>Charts, images &amp; final reports</span>
            </div>
          </div>
        </>
      )}
    </aside>
  );
});
