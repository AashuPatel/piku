export interface AppUsage {
  appId: number;
  exeName: string;
  displayName: string;
  activeSecs: number;
  iconDataUrl: string | null;
}

export interface DaySummary {
  date: string; // YYYY-MM-DD
  totalSecs: number;
  apps: AppUsage[];
  hourlySecs: number[]; // length 24 (local hours 0..23)
}

export interface DailyTotal {
  date: string; // YYYY-MM-DD
  totalSecs: number;
}

export interface AppSettings {
  trackingPaused: boolean;
  petVisible: boolean;
  petX: number;
  petY: number;
  hasSavedPosition: boolean;
  petSize: number;
  idleThresholdSecs: number;
  cuteMomentsEnabled: boolean;
  cuteFrequency: "low" | "normal" | "off";
  launchOnStartup: boolean;
}

export type AppSettingsPatch = Partial<Omit<AppSettings, "hasSavedPosition">>;
