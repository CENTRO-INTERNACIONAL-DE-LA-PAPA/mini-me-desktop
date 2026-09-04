import { useEffect, useState } from "react";
import { hex } from "../theme/theme";
import { useTheme } from "../theme/ThemeProvider";

const FRAMES = ["⠋", "⠙", "⠹", "⠸"];

export function Spinner({ colour }: { colour?: number }) {
  const { theme } = useTheme();
  const [frame, setFrame] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setFrame((f) => (f + 1) % FRAMES.length), 300);
    return () => clearInterval(id);
  }, []);

  return (
    <div style={{ flex: "none", color: hex(colour ?? theme.accent), fontSize: 13 }}>
      {FRAMES[frame]}
    </div>
  );
}
