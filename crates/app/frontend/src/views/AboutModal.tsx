import { Actions, Button, Label, Modal } from "../components";
import { useAppStore } from "../lib/store";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

const SOURCES: [string, string][] = [
  ["Asta", "Allen Institute for AI — federated academic literature search and citation tracing."],
  ["CIP Dataverse", "The International Potato Center's dataset catalogue, with persistent DOIs and full metadata."],
  ["AGROVOC", "FAO's multilingual agricultural vocabulary, used to normalise crop, soil and pest terminology."],
  ["Crop Ontology", "Standardised crop traits, genotypes and phenotypes, for comparability across studies."],
];

const ASTA_CITATION =
  "Rodriguez et al. AstaBench: Rigorous Benchmarking of AI Agents with a Scientific Research Suite. 2025.";

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const { theme } = useTheme();
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%", minWidth: 0 }}>
      <Label size="compact" colour={theme.textFaint}>
        {title}
      </Label>
      {children}
    </div>
  );
}

export function AboutModal({ onClose }: { onClose: () => void }) {
  const { theme } = useTheme();
  const executionLabel = useAppStore((state) => state.executionLabel);
  const runsLocally = executionLabel === "host (local)";

  return (
    <Modal
      title="About Mini-Me"
      width={640}
      onDismiss={onClose}
      body={
        <>
          <Label>
            A research workbench. A coordinator delegates to specialists that search the literature, find
            datasets, clean and analyse tabular data, build models, and write the findings up.
          </Label>

          <Section title="WHERE THE DATA COMES FROM">
            {SOURCES.map(([name, what]) => (
              <div key={name} style={{ display: "flex", flexDirection: "column", width: "100%", minWidth: 0 }}>
                <Label colour={theme.accent}>{name}</Label>
                <Label muted size="compact">
                  {what}
                </Label>
              </div>
            ))}
          </Section>

          <Section title="WHERE CODE RUNS">
            <Label colour={theme.accent}>{runsLocally ? "Runs on this machine" : "Runs in an isolated sandbox"}</Label>
            <Label muted size="compact">
              {runsLocally
                ? "Python and shell code the agent writes execute here, with your permissions, in this conversation's folder. Commands that touch your system stop for your approval first."
                : "Python and shell code the agent writes execute in an isolated sandbox rather than on this machine. Files it produces are copied back into this conversation's folder."}
            </Label>
          </Section>

          <Section title="CITING THIS WORK">
            <Label>
              Literature search is powered by Asta, from the Allen Institute for AI. If your work uses output
              produced with it, please cite AstaBench:
            </Label>
            <div
              style={{
                padding: "8px 12px",
                borderRadius: 6,
                borderLeft: `2px solid ${hex(theme.accent)}`,
                background: hex(theme.surface),
                color: hex(theme.text),
                fontSize: 13,
                userSelect: "text",
              }}
            >
              {ASTA_CITATION}
            </div>
            <Label muted size="compact">
              Generative AI produced the analysis and prose in this app. Say so in anything you publish from it,
              and have a subject-matter expert check it.
            </Label>
          </Section>
        </>
      }
      actions={
        <Actions>
          <div style={{ flexGrow: 1 }} />
          <Button onClick={onClose}>Close</Button>
        </Actions>
      }
    />
  );
}
