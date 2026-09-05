import { useState, useEffect } from "react";
import "./App.css";
import UsagePanel from "./components/UsagePanel";
import Settings from "./components/Settings";
import { listen } from "@tauri-apps/api/event";
import { useUsage } from "./hooks/useUsage";

function App() {
  const [view, setView] = useState<"usage" | "settings">("usage");
  const { summary, loading, error, refresh } = useUsage();

  useEffect(() => {
    const unlisten = listen<string>("navigate", (event) => {
      if (event.payload === "settings") {
        setView("settings");
      } else if (event.payload === "usage") {
        setView("usage");
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  return (
    <div className="app">
      {view === "usage" ? (
        <div className="usage-view">
          <div className="app-header">
            <h1 className="app-title">
              <span className="app-title-logo-wrap">
                <img src="/tray-logo.png" alt="" className="app-title-logo" />
              </span>
              用量
            </h1>
            <div className="header-actions">
              {summary?.last_updated && (
                <span className="header-time">{summary.last_updated}</span>
              )}
              <button className="header-refresh-btn" onClick={refresh} title="刷新数据">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="23 4 23 10 17 10"></polyline>
                  <polyline points="1 20 1 14 7 14"></polyline>
                  <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
                </svg>
              </button>
              <button className="settings-icon-btn" onClick={() => setView("settings")} title="设置">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="12" cy="12" r="3"></circle>
                  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
                </svg>
              </button>
            </div>
          </div>
          <UsagePanel summary={summary} loading={loading} error={error} refresh={refresh} />
        </div>
      ) : (
        <Settings onBack={() => setView("usage")} />
      )}
    </div>
  );
}

export default App;
