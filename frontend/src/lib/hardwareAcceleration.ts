import { invoke } from '@tauri-apps/api/core';

export type AccelerationMode = 'auto' | 'cpu';

export interface AccelerationStatus {
  mode: AccelerationMode;
  /** Speech recognition was switched to the CPU after a GPU crash */
  transcription_gpu_crashed: boolean;
  /** Built-in summaries were switched to the CPU after a GPU crash */
  summary_gpu_crashed: boolean;
  crash_notice_pending: boolean;
  /** GPU backend speech recognition was built with, e.g. "Vulkan", "Metal", or "Cpu" */
  backend: string;
  gpu: { name: string; description: string; integrated: boolean } | null;
}

export const HardwareAccelerationAPI = {
  getStatus: () => invoke<AccelerationStatus>('get_acceleration_status'),
  /** Setting a mode also clears crash fallbacks, so 'auto' means "try the GPU again" */
  setMode: (mode: AccelerationMode) => invoke<void>('set_acceleration_mode', { mode }),
  acknowledgeNotice: () => invoke<void>('acknowledge_acceleration_notice'),
};

/** Human-readable list of the engines that fell back to the CPU after a crash */
export function crashedEngines(status: AccelerationStatus): string | null {
  const engines = [
    status.transcription_gpu_crashed && 'speech recognition',
    status.summary_gpu_crashed && 'summaries',
  ].filter(Boolean);
  return engines.length ? engines.join(' and ') : null;
}
