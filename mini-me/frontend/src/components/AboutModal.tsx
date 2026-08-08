import {
  BookMarked,
  Bot,
  BookOpen,
  Brain,
  Database,
  FileSearch,
  LineChart,
  ScrollText,
  Sparkles,
  TrendingUp,
  Wrench,
  X,
  type LucideIcon,
} from "lucide-react";
import { useEffect } from "react";
import { branding } from "../branding";

interface AboutModalProps {
  onClose: () => void;
}

interface SubagentEntry {
  icon: LucideIcon;
  name: string;
  blurb: string;
}

const SUBAGENTS: SubagentEntry[] = [
  {
    icon: BookOpen,
    name: "Academic Researcher",
    blurb:
      "Searches and synthesizes peer-reviewed literature via the Asta scientific search.",
  },
  {
    icon: Database,
    name: "Dataverse Explorer",
    blurb:
      "Searches the CIP Dataverse for relevant datasets and inspects their metadata.",
  },
  {
    icon: Wrench,
    name: "Data Cleaner",
    blurb:
      "Validates schemas and harmonizes units and categories with the AGROVOC and Crop Ontology vocabularies.",
  },
  {
    icon: LineChart,
    name: "Exploratory Data Analysis",
    blurb:
      "Profiles datasets, runs descriptive statistics, and surfaces patterns — what happened?",
  },
  {
    icon: Brain,
    name: "Diagnostic Analytics",
    blurb:
      "Explains outcomes via regression, confounding checks, and group comparisons — why it happened?",
  },
  {
    icon: TrendingUp,
    name: "Predictive Analytics",
    blurb:
      "Selects and trains forecasting and prediction models with performance comparison — what will happen?",
  },
  {
    icon: ScrollText,
    name: "Report Writer",
    blurb:
      "Synthesizes findings into polished markdown reports with proper citations and uncertainty preserved.",
  },
];

const SOURCES = [
  {
    name: "Asta",
    desc: "Allen Institute for AI — federated academic literature search and citation tracing.",
  },
  {
    name: "CIP Dataverse",
    desc: "International Potato Center's dataset catalog with persistent DOIs and full metadata.",
  },
  {
    name: "AGROVOC",
    desc: "FAO's multilingual agricultural vocabulary used to normalize crop, soil, and pest terminology.",
  },
  {
    name: "Crop Ontology",
    desc: "Standardized crop traits, genotypes, and phenotypes for cross-study comparability.",
  },
];

export function AboutModal({ onClose }: AboutModalProps) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="image-lightbox about-modal"
      role="dialog"
      aria-modal="true"
      aria-label={`About ${branding.appName}`}
      onClick={onClose}
    >
      <div
        className="image-lightbox-inner about-modal-inner"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="image-lightbox-header">
          <span>{`About ${branding.appName}`}</span>
          <div className="image-lightbox-actions">
            <button
              type="button"
              className="image-lightbox-close"
              aria-label="Close"
              onClick={onClose}
            >
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="about-modal-body">
          <section>
            <p className="about-lede">{branding.about.lede}</p>
          </section>

          <section>
            <h3>
              <Sparkles size={16} aria-hidden="true" />
              The agent team
            </h3>
            <ul className="about-agents">
              {SUBAGENTS.map(({ icon: Icon, name, blurb }) => (
                <li key={name}>
                  <Icon size={16} aria-hidden="true" />
                  <div>
                    <strong>{name}</strong>
                    <p>{blurb}</p>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h3>
              <FileSearch size={16} aria-hidden="true" />
              Knowledge &amp; data sources
            </h3>
            <ul className="about-sources">
              {SOURCES.map(({ name, desc }) => (
                <li key={name}>
                  <strong>{name}</strong> — {desc}
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h3>
              <Bot size={16} aria-hidden="true" />
              Sandbox execution
            </h3>
            <p>
              Each conversation runs in an isolated LangSmith Sandbox where
              Python and shell code execute safely. Generated plots, data
              files, and reports stream back into the workspace as artifact
              cards alongside their citations.
            </p>
          </section>

          <section>
            <h3>
              <BookMarked size={16} aria-hidden="true" />
              Acknowledgements
            </h3>
            <p>
              Academic literature search is powered by <strong>Asta</strong>,
              the scientific research agent suite from the{" "}
              <strong>Allen Institute for AI</strong>. If your work uses
              output produced with Asta, please cite the AstaBench paper:
            </p>
            <p className="about-citation">
              <em>
                AstaBench: Rigorous Benchmarking of AI Agents with a
                Scientific Research Suite
              </em>{" "}
              · arXiv:2510.21652 ·{" "}
              <a
                href="https://arxiv.org/abs/2510.21652"
                target="_blank"
                rel="noreferrer"
              >
                arxiv.org/abs/2510.21652
              </a>
            </p>
          </section>

          <section className="about-attribution">
            <p>{branding.about.attribution}</p>
          </section>
        </div>
      </div>
    </div>
  );
}
