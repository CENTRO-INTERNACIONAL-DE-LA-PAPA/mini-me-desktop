const NAMES: Record<string, string> = {
  academic_researcher: "Academic Researcher",
  dataverse_explorer: "Dataverse Explorer",
  data_cleaning: "Data Cleaning",
  exploratory_data_analysis: "Exploratory Data Analysis",
  diagnostic_analytics: "Diagnostic Analytics",
  predictive_analytics: "Predictive Analytics",
  report_writer: "Report Writer",
  hypothesis_generator: "Hypothesis Generator",
  pdf_librarian: "PDF Librarian",
  data_voyager: "Data Voyager",
  autodiscovery: "AutoDiscovery",
  research_planner: "Research Planner",
};

const COLOURS: Record<string, number> = {
  academic_researcher: 0xf47920,
  dataverse_explorer: 0x20adf4,
  data_cleaning: 0xf42091,
  exploratory_data_analysis: 0x8b5cf6,
  diagnostic_analytics: 0x22c55e,
  predictive_analytics: 0xeab308,
  report_writer: 0x06b6d4,
  hypothesis_generator: 0xef4444,
  pdf_librarian: 0x14b8a6,
  data_voyager: 0xec4899,
  autodiscovery: 0xf97316,
  research_planner: 0x3b82f6,
};

export function subagentDisplay(name: string): { label: string; colour: number } {
  return { label: NAMES[name] ?? name, colour: COLOURS[name] ?? 0xffffff };
}
