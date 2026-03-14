import { useState, useRef, useCallback } from "react";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Plus, X } from "lucide-react";

interface ProcessPanelProps {
  title: string;
  subtitle: string;
  items: string[];
  onAdd: (name: string) => void;
  onRemove: (index: number) => void;
}

export function ProcessPanel({
  title,
  subtitle,
  items,
  onAdd,
  onRemove,
}: ProcessPanelProps) {
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const handleAdd = useCallback(() => {
    const trimmed = value.trim();
    if (trimmed) {
      onAdd(trimmed);
      setValue("");
      inputRef.current?.focus();
    }
  }, [value, onAdd]);

  return (
    <div className="flex-1 flex flex-col px-4 pt-3 pb-2 min-h-0">
      <div className="mb-2">
        <span className="text-[10px] text-muted-foreground tracking-widest uppercase">
          {title}
        </span>
        <span className="text-[10px] text-muted-foreground/40 ml-2">
          {subtitle}
        </span>
      </div>

      <div className="flex gap-1.5 mb-2">
        <Input
          ref={inputRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          placeholder="process.exe"
          spellCheck={false}
          className="h-7 text-[12px] bg-muted/40 border-border placeholder:text-muted-foreground/30 focus-visible:ring-1 focus-visible:ring-muted-foreground/20"
        />
        <button
          onClick={handleAdd}
          className="h-7 w-7 shrink-0 flex items-center justify-center rounded-md border border-border text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
        >
          <Plus size={13} strokeWidth={1.5} />
        </button>
      </div>

      <ScrollArea className="flex-1 min-h-0">
        {items.length === 0 ? (
          <p className="text-[11px] text-muted-foreground/30 text-center py-8">
            no entries
          </p>
        ) : (
          <div className="space-y-px">
            {items.map((item, i) => (
              <div
                key={`${item}-${i}`}
                className="flex items-center justify-between h-7 px-2.5 rounded bg-muted/30 hover:bg-muted/60 transition-colors group"
              >
                <span className="text-[12px] text-foreground/80">
                  {item}
                </span>
                <button
                  onClick={() => onRemove(i)}
                  className="w-4 h-4 flex items-center justify-center rounded-sm opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-foreground transition-all"
                >
                  <X size={11} strokeWidth={1.5} />
                </button>
              </div>
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}
