import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { LogEntry } from "@/types";

interface LogPanelProps {
  logs: LogEntry[];
  count: number;
}

export function LogPanel({ logs, count }: LogPanelProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const prevCountRef = useRef(0);

  useEffect(() => {
    if (count > prevCountRef.current) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
    prevCountRef.current = count;
  }, [count]);

  return (
    <div className="h-[170px] flex flex-col border-t border-border shrink-0">
      <div className="flex items-center px-4 h-8 border-b border-border shrink-0">
        <span className="text-[10px] text-muted-foreground tracking-widest uppercase">
          log
        </span>
        <span className="ml-2 text-[10px] text-muted-foreground/40 tabular-nums">
          {count}
        </span>
        <div className="flex-1" />
        {count > 0 && (
          <button
            onClick={() => invoke("clear_logs")}
            className="text-[10px] text-muted-foreground/50 hover:text-foreground transition-colors tracking-wider uppercase"
          >
            clear
          </button>
        )}
      </div>

      <ScrollArea className="flex-1 min-h-0">
        <div className="px-4 py-1">
          {logs.map((log, i) => (
            <div key={i} className="flex gap-3 leading-5">
              <span className="text-[11px] text-muted-foreground/30 shrink-0">
                {log.time}
              </span>
              <span
                className={cn(
                  "text-[11px] break-all",
                  log.level === "info" && "text-muted-foreground/70",
                  log.level === "success" && "text-primary/80",
                  log.level === "warning" && "text-amber-400/80"
                )}
              >
                {log.message}
              </span>
            </div>
          ))}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>
    </div>
  );
}
