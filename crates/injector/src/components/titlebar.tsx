import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, X } from "lucide-react";

export function Titlebar() {
  const appWindow = getCurrentWindow();

  return (
    <div className="h-9 flex items-center border-b border-border shrink-0">
      <div
        className="flex-1 h-full flex items-center pl-4 gap-2.5 cursor-default"
        onMouseDown={() => appWindow.startDragging()}
      >
        <span className="text-[11px] text-muted-foreground tracking-widest uppercase">
          negative
        </span>
      </div>
      <div className="flex h-full">
        <button
          onClick={() => appWindow.minimize()}
          className="w-11 h-full flex items-center justify-center text-muted-foreground/60 hover:text-foreground hover:bg-muted/60 transition-colors"
        >
          <Minus size={13} strokeWidth={1.5} />
        </button>
        <button
          onClick={() => appWindow.close()}
          className="w-11 h-full flex items-center justify-center text-muted-foreground/60 hover:text-foreground hover:bg-muted transition-colors"
        >
          <X size={13} strokeWidth={1.5} />
        </button>
      </div>
    </div>
  );
}
