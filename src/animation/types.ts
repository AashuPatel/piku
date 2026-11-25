export type AnimationStateName =
  | "idle"
  | "blink"
  | "sleep"
  | "wave"
  | "happy"
  | "click"
  | "excited"
  | "content"
  | "blush"
  | "curious"
  | "thinking"
  | "surprised"
  | "focused"
  | "yawn"
  | "drag"
  | "working"
  | "break"
  | "late_night"
  | "celebration"
  | "snack"
  | "tea"
  | "cloud"
  | "stretch"
  | "peek";

export type BaseAnimationState = "idle" | "sleep" | "late_night";

export interface AnimationStateConfig {
  frames: number;
  fps: number;
  loop: boolean;
  path: string;
  next?: AnimationStateName;
}

export interface CharacterManifest {
  character: string;
  version: string;
  assetKind: "placeholder" | "production";
  frameSize: {
    width: number;
    height: number;
  };
  anchor: {
    x: number;
    y: number;
  };
  defaultState: AnimationStateName;
  states: Record<AnimationStateName, AnimationStateConfig>;
}

export interface FrameSnapshot {
  state: AnimationStateName;
  frameIndex: number; // 0-based
  frameCount: number;
  fps: number;
  bitmap: ImageBitmap | HTMLImageElement | null;
}

export interface CuteMoment {
  animation: AnimationStateName;
  message: string;
}
