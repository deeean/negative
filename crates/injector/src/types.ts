export interface AppState {
  running: boolean;
  inject: string[];
  hide: string[];
  injected_count: number;
  failed_count: number;
  logs: LogEntry[];
  has_dll: boolean;
  has_32bit: boolean;
}

export interface LogEntry {
  time: string;
  message: string;
  level: "info" | "success" | "warning";
}
