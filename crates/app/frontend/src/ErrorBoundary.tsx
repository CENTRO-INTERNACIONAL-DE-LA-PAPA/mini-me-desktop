import { Component, type ErrorInfo, type ReactNode } from "react";

interface State {
  error: Error | null;
  info: string | null;
}

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Uncaught render error:", error, info);
    this.setState({ info: info.componentStack ?? null });
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            padding: 24,
            fontFamily: "monospace",
            fontSize: 13,
            color: "#ff6b6b",
            background: "#1a1a1a",
            height: "100vh",
            overflow: "auto",
            whiteSpace: "pre-wrap",
          }}
        >
          <h2 style={{ color: "#ff6b6b" }}>Something crashed</h2>
          <div>{this.state.error.message}</div>
          <div style={{ marginTop: 16, color: "#999" }}>{this.state.error.stack}</div>
          {this.state.info && (
            <div style={{ marginTop: 16, color: "#999" }}>Component stack:{this.state.info}</div>
          )}
        </div>
      );
    }
    return this.props.children;
  }
}
