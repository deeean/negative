import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Play, Square } from "lucide-react";
import { cn } from "@/lib/utils";
import type { AppState } from "@/types";

interface ToolbarProps {
  state: AppState;
}

export function Toolbar({ state }: ToolbarProps) {
  const canRun = state.has_dll || state.has_32bit;

  return (
    <div className="h-10 flex items-center px-4 gap-3 border-b border-border shrink-0">
      {canRun ? (
        state.running ? (
          <Button
            size="sm"
            onClick={() => invoke("stop_injection")}
            className="h-6 gap-1.5 px-3 text-[11px] bg-muted hover:bg-muted/80 text-foreground"
          >
            <Square size={10} />
            Stop
          </Button>
        ) : (
          <Button
            size="sm"
            onClick={() => invoke("start_injection")}
            className="h-6 gap-1.5 px-3 text-[11px] bg-primary text-primary-foreground hover:bg-primary/80"
          >
            <Play size={10} />
            Start
          </Button>
        )
      ) : (
        <Button size="sm" disabled className="h-6 px-3 text-[11px]" variant="secondary">
          No DLLs
        </Button>
      )}

      <span
        className={cn(
          "text-[10px] tracking-wider uppercase",
          !canRun && "text-muted-foreground",
          canRun && state.running && "text-primary",
          canRun && !state.running && "text-muted-foreground"
        )}
      >
        {!canRun ? "no dlls" : state.running ? "running" : "stopped"}
      </span>

      <div className="flex-1" />

      <span className="text-[11px] text-muted-foreground tabular-nums">
        injected: {state.injected_count}
      </span>

      {state.failed_count > 0 && (
        <span className="text-[11px] text-amber-400 tabular-nums">
          failed: {state.failed_count}
        </span>
      )}
    </div>
  );
}
