import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

type WindowErrorBoundaryState = {
  error: Error | null;
};

class WindowErrorBoundary extends React.Component<
  React.PropsWithChildren,
  WindowErrorBoundaryState
> {
  state: WindowErrorBoundaryState = {
    error: null,
  };

  static getDerivedStateFromError(error: Error): WindowErrorBoundaryState {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <main className="incoming-window">
          <h1>窗口加载失败</h1>
          <p>{this.state.error.message || "发生了未预期的前端错误。"}</p>
        </main>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WindowErrorBoundary>
      <App />
    </WindowErrorBoundary>
  </React.StrictMode>,
);
