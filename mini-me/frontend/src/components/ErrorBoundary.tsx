import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  handleReset = () => {
    this.setState({ error: null });
  };

  render() {
    if (this.state.error) {
      return (
        this.props.fallback ?? (
          <div className="error-boundary" role="alert">
            <h2>Something went wrong</h2>
            <p>{this.state.error.message}</p>
            <button type="button" onClick={this.handleReset}>
              Try again
            </button>
          </div>
        )
      );
    }
    return this.props.children;
  }
}
