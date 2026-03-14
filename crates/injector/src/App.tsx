import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Titlebar } from "@/components/titlebar";
import { Toolbar } from "@/components/toolbar";
import { ProcessPanel } from "@/components/process-panel";
import { LogPanel } from "@/components/log-panel";
import { Separator } from "@/components/ui/separator";
import type { AppState } from "@/types";

const STORAGE_KEY = "negative-config";

function loadConfig(): { inject: string[]; hide: string[] } {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch {}
  return { inject: [], hide: [] };
}

function saveConfig(inject: string[], hide: string[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ inject, hide }));
}

export default function App() {
  const [state, setState] = useState<AppState | null>(null);
  const initialized = useRef(false);

  useEffect(() => {
    const config = loadConfig();
    invoke("init_config", { config })
      .catch(() => {})
      .finally(() => {
        invoke<AppState>("get_state").then(setState);
        initialized.current = true;
      });

    const unlisten = listen<AppState>("state-update", (e) => {
      setState(e.payload);
      if (initialized.current) {
        saveConfig(e.payload.inject, e.payload.hide);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (!state) return null;

  return (
    <div className="h-screen flex flex-col overflow-hidden">
      <Titlebar />
      <Toolbar state={state} />
      <div className="flex-1 flex min-h-0">
        <ProcessPanel
          title="Inject"
          subtitle="Processes to inject into"
          items={state.inject}
          onAdd={(name) => invoke("add_inject", { name })}
          onRemove={(index) => invoke("remove_inject", { index })}
        />
        <Separator orientation="vertical" />
        <ProcessPanel
          title="Hide"
          subtitle="Processes to hide"
          items={state.hide}
          onAdd={(name) => invoke("add_hide", { name })}
          onRemove={(index) => invoke("remove_hide", { index })}
        />
      </div>
      <LogPanel logs={state.logs} count={state.logs.length} />
    </div>
  );
}
