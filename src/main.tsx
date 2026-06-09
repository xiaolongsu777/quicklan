import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

function showBootError(message: string) {
  const pre = document.getElementById("boot-error");
  const text = document.getElementById("boot-text");
  if (text) {
    text.textContent = "QuickLAN 窗口启动失败";
  }
  if (pre) {
    pre.textContent = message;
  }
}

class RootErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: string | null }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error: unknown) {
    return {
      error: error instanceof Error ? error.stack || error.message : String(error),
    };
  }

  componentDidCatch(error: unknown) {
    console.error("[boot] root render error", error);
    showBootError(error instanceof Error ? error.stack || error.message : String(error));
  }

  render() {
    return this.state.error ? null : this.props.children;
  }
}

try {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <RootErrorBoundary>
        <App />
      </RootErrorBoundary>
    </React.StrictMode>,
  );
} catch (error) {
  console.error("[boot] fatal startup error", error);
  showBootError(error instanceof Error ? error.stack || error.message : String(error));
}
