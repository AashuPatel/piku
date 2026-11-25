import type { SpriteStore } from "./spriteStore";
import type {
  AnimationStateName,
  BaseAnimationState,
  CuteMoment,
  FrameSnapshot,
} from "./types";

type FrameListener = (snapshot: FrameSnapshot) => void;
type CuteMomentListener = (moment: CuteMoment) => void;

const AMBIENT_MOMENTS: readonly CuteMoment[] = [
  { animation: "peek", message: "psst..." },
  { animation: "cloud", message: "I found a pretty cloud today ☁️" },
  { animation: "wave", message: "Hehe, hi! ♡" },
  { animation: "snack", message: "It's snack time. I don't make the rules." },
  { animation: "tea", message: "Cozy sip of tea? 🍵" },
  { animation: "stretch", message: "Big stretch! Feels nice, right?" },
  { animation: "working", message: "Typing away right alongside you~" },
  { animation: "break", message: "Vibing to a cozy little tune ♪" },
  { animation: "focused", message: "We've got this!" },
  { animation: "thinking", message: "Hmm... wondering what's next." },
  { animation: "curious", message: "Ooh, what are we looking at?" },
  { animation: "content", message: "Just happy sitting here with you ♡" },
  { animation: "blush", message: "Hehe, cozy vibes today~" },
  { animation: "excited", message: "Yay! Good energy right now!" },
  { animation: "celebration", message: "Little cheer for your day!" },
  { animation: "surprised", message: "Oh! Did you see that?" },
  { animation: "happy", message: "Hope your day feels gentle ♡" },
];

const BLINK_MIN_MS = 3500;
const BLINK_MAX_MS = 7500;
const CUTE_MIN_MS = 4 * 60 * 1000; // 4 minutes
const CUTE_MAX_MS = 8 * 60 * 1000; // 8 minutes

export interface ControllerOptions {
  enableRandomBlink?: boolean;
  enableAmbientMoments?: boolean;
  initialBaseState?: BaseAnimationState;
}

function isPassiveState(state: AnimationStateName): boolean {
  return (
    state === "idle" ||
    state === "sleep" ||
    state === "late_night" ||
    state === "blink"
  );
}

/**
 * Pure timer-driven animation FSM (no 60fps requestAnimationFrame polling).
 * Emits frame updates ONLY when the visible frame index actually changes.
 */
export class AnimationController {
  private readonly store: SpriteStore;
  private readonly enableRandomBlink: boolean;
  private enableAmbientMoments: boolean;

  private baseState: BaseAnimationState;
  private currentState: AnimationStateName;
  private frameIndex = 0;
  private blinkQueued = false;
  private suspended = false;
  private dragging = false;
  private preDragSnapshot: {
    state: AnimationStateName;
    frameIndex: number;
    baseState: BaseAnimationState;
  } | null = null;
  private lastMomentIdx = -1;
  private transitionSeq = 0;

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
    if (this.dragging) {
      return;
    }
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

  public setBaseState(
    nextBase: BaseAnimationState,
    options: { transition?: AnimationStateName } = {}
  ): void {
    if (this.baseState === nextBase) return;
    const prevBase = this.baseState;
    this.baseState = nextBase;
    this.blinkQueued = false;

    // While dragging, record the new authoritative baseState and keep static drag_03 visible until release.
    if (this.dragging) {
      return;
    }

    // Immediately switch if currently in a passive state; one-shots finish cleanly then settle to baseState
    if (isPassiveState(this.currentState)) {
      if (
        options.transition &&
        prevBase === "idle" &&
        nextBase === "sleep" &&
        options.transition in this.store.manifest.states
      ) {
        this.transitionTo(options.transition);
      } else {
        this.transitionTo(nextBase);
      }
    }

    if (nextBase !== "idle") {
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
   *  - Active drag holds static drag_03.png until endDrag() and cannot be interrupted.
   *  - "click" interrupts non-drag states immediately.
   *  - Other expressive one-shots won't override an active click or drag.
   *  - "blink" never interrupts non-idle states.
   */
  public play(state: AnimationStateName): void {
    if (this.dragging) return;
    if (!(state in this.store.manifest.states)) return;

    if (state === "blink") {
      if (this.currentState !== "idle") return;
      this.transitionTo("blink");
      return;
    }

    if (
      state !== "click" &&
      state !== "drag" &&
      (this.currentState === "click" || this.currentState === "drag")
    ) {
      return;
    }

    if (state === "sleep" || state === "late_night" || state === "idle") {
      this.baseState = state;
    }

    this.blinkQueued = false;
    this.transitionTo(state);
  }

  /**
   * Enters static drag hold showing ONLY drag_03.png (0-based frameIndex = 2) with zero looping,
   * remembering the exact pre-drag state so endDrag() restores it without flicker.
   */
  public startDrag(): void {
    if (this.dragging) return;
    if (!("drag" in this.store.manifest.states)) return;

    this.preDragSnapshot = {
      state: this.currentState,
      frameIndex: this.frameIndex,
      baseState: this.baseState,
    };
    this.dragging = true;
    this.blinkQueued = false;
    const seq = ++this.transitionSeq;
    this.clearTimers();

    this.currentState = "drag";
    this.frameIndex = 2; // drag_03.png

    if (!this.store.hasStateLoaded("drag")) {
      void this.store.ensureStateLoaded("drag").then(() => {
        if (!this.dragging || this.transitionSeq !== seq || this.suspended) return;
        this.notifyFrame();
      });
    }
    this.notifyFrame();
  }

  /**
   * Exits drag immediately and restores the exact state Piku was in before dragging
   * (or the updated authoritative baseState if a state-hint arrived mid-drag).
   */
  public endDrag(): void {
    if (!this.dragging) {
      if (this.currentState === "drag") {
        this.transitionTo(this.baseState);
      }
      return;
    }

    const prev = this.preDragSnapshot;
    this.dragging = false;
    this.preDragSnapshot = null;
    this.blinkQueued = false;
    ++this.transitionSeq;
    this.clearTimers();

    if (prev && this.baseState === prev.baseState && prev.state in this.store.manifest.states) {
      const cfg = this.store.manifest.states[prev.state];
      this.currentState = prev.state;
      this.frameIndex = Math.min(Math.max(0, prev.frameIndex), Math.max(0, cfg.frames - 1));
    } else {
      this.currentState = this.baseState;
      this.frameIndex = 0;
    }

    this.notifyFrame();
    if (!this.suspended) {
      this.scheduleNextFrame();
      this.scheduleBlink();
      this.scheduleAmbientMoment();
    }
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
    if (!(state in this.store.manifest.states)) return;
    const seq = ++this.transitionSeq;

    if (!this.store.hasStateLoaded(state)) {
      void this.store.ensureStateLoaded(state).then(() => {
        if (this.transitionSeq !== seq || this.suspended) return;
        this.applyTransition(state);
      });
      return;
    }

    this.applyTransition(state);
  }

  private applyTransition(state: AnimationStateName): void {
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

    // One-shot animation finished -> always settle cleanly to the authoritative baseState
    // (prevents e.g. mid-yawn wake-up from falling back into "sleep" via manifest next)
    this.transitionTo(this.baseState);
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
