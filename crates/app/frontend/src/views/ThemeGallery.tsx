import { useState } from "react";
import { Actions, Button, Label, Modal, SearchBar, Spinner } from "../components";
import { ipc } from "../lib/ipc";
import type { GalleryListing } from "../lib/protocol";
import { useTheme } from "../theme/ThemeProvider";
import { hex } from "../theme/theme";

export function ThemeGallery({ onClose, onInstalled }: { onClose: () => void; onInstalled: (name: string) => void }) {
  const { theme, refreshInstalledThemes } = useTheme();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<GalleryListing[]>([]);
  const [searching, setSearching] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [note, setNote] = useState("");

  const search = async (q: string) => {
    setQuery(q);
    if (!q.trim()) {
      setResults([]);
      return;
    }
    setSearching(true);
    try {
      setResults(await ipc.searchThemes(q.trim()));
    } catch (error) {
      setNote(String(error));
    } finally {
      setSearching(false);
    }
  };

  const install = async (listing: GalleryListing) => {
    setInstalling(listing.id);
    try {
      const names = await ipc.installTheme(listing.id);
      refreshInstalledThemes();
      if (names[0]) onInstalled(names[0]);
      setNote(`Installed ${names.join(", ")}.`);
    } catch (error) {
      setNote(String(error));
    } finally {
      setInstalling(null);
    }
  };

  return (
    <Modal
      title="BROWSE THEMES"
      width={560}
      onDismiss={onClose}
      body={
        <>
          <SearchBar value={query} placeholder="Search Zed's theme gallery…" onChange={search} />
          {searching && <Spinner />}
          {note && (
            <Label size="compact" muted>
              {note}
            </Label>
          )}
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {results.map((listing) => (
              <div
                key={listing.id}
                style={{
                  display: "flex",
                  flexDirection: "row",
                  justifyContent: "space-between",
                  alignItems: "center",
                  padding: 8,
                  borderRadius: 6,
                  border: `1px solid ${hex(theme.border)}`,
                }}
              >
                <div style={{ display: "flex", flexDirection: "column", minWidth: 0, gap: 2 }}>
                  <Label ellipsis>{listing.name}</Label>
                  <Label muted size="compact" ellipsis>
                    {listing.authors.join(", ")} · {listing.download_count.toLocaleString()} installs
                  </Label>
                </div>
                <Button onClick={() => install(listing)} disabled={installing === listing.id}>
                  {installing === listing.id ? "Installing…" : "Install"}
                </Button>
              </div>
            ))}
          </div>
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
