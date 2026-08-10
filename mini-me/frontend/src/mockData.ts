import type {
  DatasetArtifact,
  ReportArtifact,
  SourceArtifact,
  SubagentRun,
  TodoItem,
} from "./types";

export const mockTodos: TodoItem[] = [
  {
    status: "completed",
    content: "Search CIP Dataverse for late blight datasets",
  },
  {
    status: "in_progress",
    content: "Inspect shortlisted metadata and file accessibility",
  },
  {
    status: "pending",
    content: "Summarize candidate datasets for analysis",
  },
];

export const mockSubagents: SubagentRun[] = [
  {
    id: "subagent-1",
    name: "dataverse_explorer",
    label: "Dataverse Explorer",
    status: "completed",
    task: "Find CIP Dataverse datasets related to potato late blight.",
    elapsed: "42s",
    resultPreview: "Found 3 relevant datasets with DOI links and file counts.",
  },
  {
    id: "subagent-2",
    name: "academic_researcher",
    label: "Academic Researcher",
    status: "running",
    task: "Find recent literature connected to late blight forecasting methods.",
    elapsed: "1m 08s",
    resultPreview: "Searching Asta for model validation and disease forecasting papers.",
  },
  {
    id: "subagent-3",
    name: "exploratory_data_analysis",
    label: "Exploratory Data Analysis",
    status: "queued",
    task: "Profile the selected dataset after the user confirms the source.",
    elapsed: "waiting",
  },
];

export const mockDatasets: DatasetArtifact[] = [
  {
    title:
      "Replication data for: Qualification of a Plant Disease Simulation Model",
    authors: ["International Potato Center"],
    persistentId: "doi:10.21223/P3/0F9T62",
    description:
      "Dataset supporting validation of a late blight simulation model for New York field observations.",
    repository: "CIP Dataverse",
    fileCount: 6,
    accessSummary: "Public files available; no restricted files detected.",
    recommendationReason:
      "Relevant because it links field disease observations with model qualification outputs.",
  },
];

export const mockSources: SourceArtifact[] = [
  {
    citation:
      "Forbes, G. A., et al. (2014). Using host resistance to manage potato late blight.",
    relevance:
      "Useful background for interpreting resistance and disease management variables.",
    link: "https://doi.org/example",
  },
];

export const mockReport: ReportArtifact = {
  title: "Late Blight Dataset Discovery Summary",
  markdown:
    "### Key finding\n\nA CIP Dataverse dataset with simulation model validation data is the strongest candidate for follow-up analysis.\n\n### Caveat\n\nThe dataset should be inspected for variable definitions and temporal coverage before modeling.",
};
