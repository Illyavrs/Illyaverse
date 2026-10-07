export type Client = 'stable' | 'lazer';
export type Phase = 'idle' | 'preparing' | 'running' | 'closing' | 'cleaning' | 'cleanup_failed' | 'error';
export type Compatibility = 'unknown' | 'supported' | 'unsupported';

export interface LauncherSnapshot {
  stable: { path: string | null; running: boolean };
  lazer: { path: string | null; running: boolean; compatibility: Compatibility };
  phase: Phase;
  detail: string;
  pluginPresent: boolean;
}

export type LauncherCommand = 'launcher_snapshot' | 'launcher_choose_path' | 'launcher_start' | 'launcher_stop' | 'launcher_recover';

export interface LauncherBridge {
  mode: 'native' | 'preview';
  interactive: boolean;
  call(command: LauncherCommand, args?: { client: Client }): Promise<LauncherSnapshot>;
}

const phases: Phase[] = ['idle', 'preparing', 'running', 'closing', 'cleaning', 'cleanup_failed', 'error'];
const compatibility: Compatibility[] = ['unknown', 'supported', 'unsupported'];

export const emptySnapshot: LauncherSnapshot = {
  stable: { path: null, running: false },
  lazer: { path: null, running: false, compatibility: 'unknown' },
  phase: 'idle', detail: '選擇遊戲的安裝位置，即可準備啟動。', pluginPresent: false,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function clientState(value: unknown): value is LauncherSnapshot['stable'] {
  return isRecord(value) && (typeof value.path === 'string' || value.path === null) && typeof value.running === 'boolean';
}

function lazerState(value: unknown): value is LauncherSnapshot['lazer'] {
  return isRecord(value) && compatibility.includes(value.compatibility as Compatibility) && clientState(value);
}

export function parseSnapshot(value: unknown): LauncherSnapshot {
  if (!isRecord(value) || !clientState(value.stable) || !lazerState(value.lazer)
    || !phases.includes(value.phase as Phase) || typeof value.detail !== 'string'
    || typeof value.pluginPresent !== 'boolean') {
    throw new Error('啟動器回傳的狀態無法讀取。請重新開啟啟動器。');
  }
  return value as unknown as LauncherSnapshot;
}

// This fixture never reads the filesystem, starts a process, or sends a request.
// Its actions are enabled only by the development build; production browser views are read-only.
function previewBridge(): LauncherBridge {
  const interactive = process.env.NODE_ENV === 'development';
  let snapshot = structuredClone(emptySnapshot);
  return {
    mode: 'preview', interactive,
    async call(command, args) {
      if (command === 'launcher_snapshot') return structuredClone(snapshot);
      if (!interactive) throw new Error('請使用桌面版 Kaleidoscope 啟動遊戲。');
      if (command === 'launcher_choose_path' && args) {
        if (args.client === 'stable') snapshot.stable.path = 'C:\\Games\\osu!\\osu!.exe';
        else {
          snapshot.lazer.path = 'C:\\Users\\Player\\AppData\\Local\\osulazer\\current\\osu!.exe';
          snapshot.lazer.compatibility = 'supported';
        }
      } else if (command === 'launcher_start' && args) {
        await new Promise((resolve) => setTimeout(resolve, 350));
        snapshot[args.client].running = true;
        snapshot.phase = 'running';
        snapshot.detail = `${args.client} 已啟動（預覽狀態）。`;
        snapshot.pluginPresent = snapshot.lazer.running;
      } else if (command === 'launcher_stop' || command === 'launcher_recover') {
        await new Promise((resolve) => setTimeout(resolve, 350));
        snapshot = {
          ...snapshot,
          stable: { ...snapshot.stable, running: false },
          lazer: { ...snapshot.lazer, running: false },
          phase: 'idle', detail: '遊戲已關閉，連線 ruleset 已清理（預覽狀態）。', pluginPresent: false,
        };
      }
      return structuredClone(snapshot);
    },
  };
}

export async function createLauncherBridge(): Promise<LauncherBridge> {
  const { isTauri, invoke } = await import('@tauri-apps/api/core');
  if (!isTauri()) return previewBridge();
  return {
    mode: 'native', interactive: true,
    async call(command, args) {
      return parseSnapshot(await invoke<unknown>(command, args));
    },
  };
}
