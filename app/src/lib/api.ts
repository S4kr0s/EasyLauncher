import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Project = {
  id: string;
  name: string;
  description?: string;
  icon?: string;
  manifestUrl: string;
  hub?: string;
  latestVersion?: string;
  installedVersion?: string;
};
export type HubStatus = { url: string; name?: string; error?: string };
export type Overview = { projects: Project[]; hubs: HubStatus[]; running: string[] };

export type FileEntry = { path: string; policy: string; optional?: boolean; name?: string; size?: number };
export type Pack = {
  id: string;
  name: string;
  version: string;
  description?: string;
  changelog?: string;
  minecraft?: { version: string; loader: { type: string; version?: string }; memoryMb?: number };
  servers: { name: string; address: string }[];
  files: FileEntry[];
};
export type Details = {
  pack: Pack;
  changes: number;
  kept: string[];
  downloadBytes: number;
  installedVersion?: string;
  disabledOptional: string[];
};
export type Settings = { hubs: string[]; defaultMemoryMb: number; msClientId?: string | null; jvmArgs: string[] };
export type InstanceSettings = {
  memoryMb?: number | null;
  javaPath?: string | null;
  jvmArgs: string[];
  width?: number | null;
  height?: number | null;
  autoJoin: boolean;
};
export type Account = { id: string; name: string; kind: "microsoft" | "offline"; active: boolean };
export type DeviceCode = { userCode: string; verificationUri: string; deviceCode: string; interval: number; expiresIn: number };

export type CoreEvent =
  | { kind: "stage"; text: string }
  | { kind: "progress"; done: number; total: number }
  | { kind: "log"; text: string }
  | { kind: "game"; line: string };

/** A file the player fetches in their browser; matched in Downloads by sha1. */
export type Missing = { path: string; name: string; page: string; filename: string | null; sha1: string; size: number };
export type TaskDone = { project: string; kind: "play" | "update"; error: string | null; exitCode: number | null; needsDownloads: Missing[] | null };

export const api = {
  overview: () => invoke<Overview>("overview"),
  addHub: (url: string) => invoke<string>("add_hub", { url }),
  removeHub: (url: string) => invoke<void>("remove_hub", { url }),
  details: (manifestUrl: string) => invoke<Details>("details", { manifestUrl }),
  update: (manifestUrl: string, id: string) => invoke<void>("update", { manifestUrl, id }),
  play: (manifestUrl: string, id: string) => invoke<void>("play", { manifestUrl, id }),
  setOptional: (id: string, path: string, enabled: boolean) => invoke<void>("set_optional", { id, path, enabled }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  getInstanceSettings: (id: string) => invoke<InstanceSettings>("get_instance_settings", { id }),
  saveInstanceSettings: (id: string, settings: InstanceSettings) => invoke<void>("save_instance_settings", { id, settings }),
  openFolder: (id: string) => invoke<void>("open_folder", { id }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  uninstall: (id: string) => invoke<void>("uninstall", { id }),
  listAccounts: () => invoke<{ accounts: Account[]; microsoftAvailable: boolean }>("list_accounts"),
  addOffline: (name: string) => invoke<void>("add_offline", { name }),
  selectAccount: (id: string) => invoke<void>("select_account", { id }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  msLoginStart: () => invoke<DeviceCode>("ms_login_start"),
  scanDownloads: (items: Missing[]) => invoke<string[]>("scan_downloads", { items }),
  importDownload: (path: string, items: Missing[]) => invoke<string | null>("import_download", { path, items }),
  msLoginPoll: (deviceCode: string) => invoke<boolean>("ms_login_poll", { deviceCode }),
};

export function onTask(cb: (project: string, e: CoreEvent) => void) {
  return listen<{ project: string; event: CoreEvent }>("task", (m) => cb(m.payload.project, m.payload.event));
}
export function onTaskDone(cb: (d: TaskDone) => void) {
  return listen<TaskDone>("task-done", (m) => cb(m.payload));
}

export function mb(bytes: number) {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;
}
