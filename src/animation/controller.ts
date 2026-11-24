import type { SpriteStore } from "./spriteStore";
import type { AnimationStateName, CuteMoment, FrameSnapshot } from "./types";

type FrameListener = (snapshot: FrameSnapshot) => void;
type CuteMomentListener = (moment: CuteMoment) => void;

const AMBIENT_MOMENTS: readonly CuteMoment[] = [
  { animation: "wave", message: "psst..." },
  { animation: "happy", message: "I found a pretty cloud today ☁️" },
  { animation: "wave", message: "Hehe, hi! ♡" },
  { animation: "happy", message: "It's snack time. I don't make the rules." },
  { animation: "wave", message: "You look busy. I'll nap here." },
];

const BLINK_MIN_MS = 3500;
const BLINK_MAX_MS = 7500;
const CUTE_MIN_MS = 4 * 60 * 1000; // 4 minutes
const CUTE_MAX_MS = 8 * 60 * 1000; // 8 minutes

export interface ControllerOptions {
  enableRandomBlink?: boolean;
  enableAmbientMoments?: boolean;
  initialBaseState?: "idle" | "sleep";
}

/**
 * Pure timer-driven animation FSM (no 60fps requestAnimationFrame polling).
 * Emits frame updates ONLY when the visible frame index actually changes.
 */
export class AnimationController {
  private readonly store: SpriteStore;
  private readonly enableRandomBlink: boolean;
  private enableAmbientMoments: boolean;

  private baseState: "idle" | "sleep";
  private currentState: AnimationStateName;
  private frameIndex = 0;
  private blinkQueued = false;
  private suspended = false;
  private lastMomentIdx = -1;

  private frameTimer: number | null = null;
  private blinkTimer: number | null = null;
  private momentTimer: number | null = null;

  private readonly frameListeners = new Set<FrameListener>();
  private readonly momentListeners = new Set<CuteMomentListener>();

  constructor(store: SpriteStore, options: ControllerOptions = {}) {
    this.store = store;
    this.enableRandomBlink = options.enableRandomBlink ?? true;
    this.enableAmbientMoments = options.enableAmbientMoments ?? false;
    this.baseState = options.initialBaseState ?? "idle";
    this.currentState = this.baseState;
  }

  public start(): void {
    this.suspended = false;
    this.notifyFrame();
    this.scheduleNextFrame();
    this.scheduleBlink();
    this.scheduleAmbientMoment();
  }

  public stop(): void {
    this.suspended = true;
    this.clearTimers();
  }

  public setSuspended(suspended: boolean): void {
    if (this.suspended === suspended) return;
    if (suspended) {
      this.stop();
    } else {
      this.start();
    }
  }

  public setBaseState(nextBase: "idle" | "sleep"): void {
    if (this.baseState === nextBase) return;
    this.baseState = nextBase;
    this.blinkQueued = false;

    // Immediately switch if currently in a passive state; one-shots finish cleanly then settle to baseState
    if (this.currentState === "idle" || this.currentState === "sleep" || this.currentState === "blink") {
      this.transitionTo(nextBase);
    }

    if (nextBase === "sleep") {
      this.clearBlinkTimer();
      this.clearMomentTimer();
    } else if (!this.suspended) {
      this.scheduleBlink();
      this.scheduleAmbientMoment();
    }
  }

  /**
   * Play a specific animation state.
   * Priority rules:
   *  - "click" interrupts anything immediately.
   *  - "wave" / "happy" interrupt passive states (idle/blink/sleep) but won't override an active click.
   *  - "blink" never interrupts click, wave, happy, or sleep.
   */
  public play(state: AnimationStateName): void {
    if (state === "blink") {
      if (this.currentState !== "idle") return;
      this.transitionTo("blink");
      return;
    }

    if ((state === "wave" || state === "happy") && this.currentState === "click") {
      return;
    }

    if (state === "sleep") {
      this.baseState = "sleep";
    } else if (state === "idle") {
      this.baseState = "idle";
    }

    this.blinkQueued = false;
    this.transitionTo(state);
  }

  public setAmbientMomentsEnabled(enabled: boolean): void {
    if (this.enableAmbientMoments === enabled) return;
    this.enableAmbientMoments = enabled;
    if (!enabled) {
      this.clearMomentTimer();
    } else if (!this.suspended && this.baseState === "idle") {
      this.scheduleAmbientMoment();
    }
  }

  public triggerAmbientMoment(): CuteMoment | null {
    if (
      !this.enableAmbientMoments ||
      this.suspended ||
      this.baseState !== "idle" ||
      this.currentState !== "idle"
    ) {
      return null;
    }
    let idx = Math.floor(Math.random() * AMBIENT_MOMENTS.length);
    if (idx === this.lastMomentIdx) {
      idx = (idx + 1) % AMBIENT_MOMENTS.length;
    }
    this.lastMomentIdx = idx;
    const moment = AMBIENT_MOMENTS[idx];
    this.play(moment.animation);
    for (const cb of this.momentListeners) {
      cb(moment);
    }
    return moment;
  }

  public getSnapshot(): FrameSnapshot {
    const cfg = this.store.manifest.states[this.currentState];
    return {
      state: this.currentState,
      frameIndex: this.frameIndex,
      frameCount: cfg.frames,
      fps: cfg.fps,
      bitmap: this.store.getFrame(this.currentState, this.frameIndex),
    };
  }

  public onFrameChange(listener: FrameListener): () => void {
    this.frameListeners.add(listener);
    listener(this.getSnapshot());
    return () => {
      this.frameListeners.delete(listener);
    };
  }

  public onCuteMoment(listener: CuteMomentListener): () => void {
    this.momentListeners.add(listener);
    return () => {
      this.momentListeners.delete(listener);
    };
  }

  private transitionTo(state: AnimationStateName): void {
    this.currentState = state;
    this.frameIndex = 0;
    this.clearFrameTimer();
    this.notifyFrame();
    if (!this.suspended) {
      this.scheduleNextFrame();
    }
  }

  private scheduleNextFrame(): void {
    this.clearFrameTimer();
    if (this.suspended) return;

    const cfg = this.store.manifest.states[this.currentState];
    const delayMs = Math.max(40, Math.round(1000 / cfg.fps));

    this.frameTimer = window.setTimeout(() => {
      this.frameTimer = null;
      this.advanceFrame();
    }, delayMs);
  }

  private advanceFrame(): void {
    if (this.suspended) return;

    const cfg = this.store.manifest.states[this.currentState];
    const nextIndex = this.frameIndex + 1;

    if (nextIndex < cfg.frames) {
      this.frameIndex = nextIndex;
      this.notifyFrame();
      this.scheduleNextFrame();
      return;
    }

    // Reached end of clip
    if (cfg.loop) {
      // Trigger queued blink cleanly right at neutral idle loop boundary (0px body pop)
      if (this.currentState === "idle" && this.blinkQueued && this.baseState === "idle") {
        this.blinkQueued = false;
        this.transitionTo("blink");
        return;
      }
      this.frameIndex = 0;
      this.notifyFrame();
      this.scheduleNextFrame();
      return;
    }

    // One-shot animation finished -> return cleanly to baseState (or manifest next)
    const fallbackNext: AnimationStateName =
      this.baseState === "sleep" ? "sleep" : cfg.next ?? this.baseState;
    this.transitionTo(fallbackNext);
  }

  private scheduleBlink(): void {
    this.clearBlinkTimer();
    if (!this.enableRandomBlink || this.suspended || this.baseState !== "idle") return;

    const waitMs = BLINK_MIN_MS + Math.random() * (BLINK_MAX_MS - BLINK_MIN_MS);
    this.blinkTimer = window.setTimeout(() => {
      this.blinkTimer = null;
      if (!this.suspended && this.baseState === "idle" && this.currentState === "idle") {
        this.blinkQueued = true;
      }
      this.scheduleBlink();
    }, waitMs);
  }

  private scheduleAmbientMoment(): void {
    this.clearMomentTimer();
    if (!this.enableAmbientMoments || this.suspended || this.baseState !== "idle") return;

    const waitMs = CUTE_MIN_MS + Math.random() * (CUTE_MAX_MS - CUTE_MIN_MS);
    this.momentTimer = window.setTimeout(() => {
      this.momentTimer = null;
      this.triggerAmbientMoment();
      this.scheduleAmbientMoment();
    }, waitMs);
  }

  private notifyFrame(): void {
    const snap = this.getSnapshot();
    for (const cb of this.frameListeners) {
      cb(snap);
    }
  }

  private clearFrameTimer(): void {
    if (this.frameTimer !== null) {
      window.clearTimeout(this.frameTimer);
      this.frameTimer = null;
    }
  }

  private clearBlinkTimer(): void {
    if (this.blinkTimer !== null) {
      window.clearTimeout(this.blinkTimer);
      this.blinkTimer = null;
    }
  }

  private clearMomentTimer(): void {
    if (this.momentTimer !== null) {
      window.clearTimeout(this.momentTimer);
      this.momentTimer = null;
    }
  }

  private clearTimers(): void {
    this.clearFrameTimer();
    this.clearBlinkTimer();
    this.clearMomentTimer();
  }
}
