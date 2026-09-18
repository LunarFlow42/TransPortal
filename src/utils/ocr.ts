import type { OcrEvent, OcrStage } from '../types';

const STAGE_ORDER: Record<OcrStage, number> = {
  capturing: 0,
  recognizing: 1,
  ready: 2,
  cancelled: 3,
  error: 2,
};

/** IPC events and startup replay can arrive out of order. Terminal events win. */
export function isNewOcrEvent(event: OcrEvent, previous: OcrEvent | null): boolean {
  if (!previous) return true;
  if (event.id !== previous.id) return event.id > previous.id;
  return STAGE_ORDER[event.stage] > STAGE_ORDER[previous.stage];
}
