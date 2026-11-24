import type { AnimationStateName, CharacterManifest } from "./types";

export type DrawableSprite = ImageBitmap | HTMLImageElement;

const MANIFEST_URL = "/assets/character/piku/manifest.json";
const ASSET_BASE = "/assets/character/piku/";
const HIT_MASK_SIZE = 64;

export interface SpriteLoadOptions {
  /** Optional subset of animation states to preload (always includes manifest.defaultState). */
  states?: readonly AnimationStateName[];
  /** Whether to build the 64x64 alpha silhouette hit-test mask (default true). */
  buildHitMask?: boolean;
}

export class SpriteStore {
  public readonly manifest: CharacterManifest;
  private readonly frames = new Map<AnimationStateName, (DrawableSprite | null)[]>();
  private fallbackFrame: DrawableSprite | null = null;
  private hitMask: Uint8Array | null = null;

  private constructor(manifest: CharacterManifest) {
    this.manifest = manifest;
  }

  public static async load(options: SpriteLoadOptions = {}): Promise<SpriteStore> {
    const res = await fetch(MANIFEST_URL);
    if (!res.ok) {
      throw new Error(`Failed to load Piku manifest (${res.status})`);
    }
    const manifest = (await res.json()) as CharacterManifest;
    const store = new SpriteStore(manifest);
    await store.preloadStates(options.states, options.buildHitMask ?? true);
    return store;
  }

  private async preloadStates(
    requestedStates: readonly AnimationStateName[] | undefined,
    shouldBuildHitMask: boolean
  ): Promise<void> {
    const allStates = Object.keys(this.manifest.states) as AnimationStateName[];
    const stateNames = requestedStates
      ? Array.from(
          new Set<AnimationStateName>([
            this.manifest.defaultState,
            ...requestedStates.filter((s) => s in this.manifest.states),
          ])
        )
      : allStates;

    await Promise.all(
      stateNames.map(async (stateName) => {
        const cfg = this.manifest.states[stateName];
        const urls: string[] = [];
        for (let i = 1; i <= cfg.frames; i++) {
          const nn = String(i).padStart(2, "0");
          urls.push(`${ASSET_BASE}${cfg.path.replace("{nn}", nn)}`);
        }

        const decoded = await Promise.all(urls.map((url) => this.loadSprite(url)));
        this.frames.set(stateName, decoded);
      })
    );

    const idleFrames = this.frames.get(this.manifest.defaultState);
    this.fallbackFrame = idleFrames?.[0] ?? null;

    if (shouldBuildHitMask && this.fallbackFrame) {
      this.hitMask = buildHitMask(this.fallbackFrame);
    }
  }

  private async loadSprite(url: string): Promise<DrawableSprite | null> {
    try {
      const response = await fetch(url);
      if (!response.ok) {
        console.warn(`[Piku SpriteStore] Missing frame: ${url} (${response.status})`);
        return null;
      }
      const blob = await response.blob();
      if (typeof createImageBitmap === "function") {
        return await createImageBitmap(blob);
      }
      return await loadHtmlImage(url);
    } catch (err) {
      console.warn(`[Piku SpriteStore] Failed decoding ${url}:`, err);
      return null;
    }
  }

  public getFrame(state: AnimationStateName, frameIndex: number): DrawableSprite | null {
    const list = this.frames.get(state);
    if (!list || list.length === 0) {
      return this.fallbackFrame;
    }
    const safeIdx = ((frameIndex % list.length) + list.length) % list.length;
    return list[safeIdx] ?? this.fallbackFrame;
  }

  /**
   * Returns true if normalized (u, v) in [0, 1] hits Piku's silhouette (with a soft 3px forgiveness pad).
   */
  public hitTest(u: number, v: number): boolean {
    if (u < 0 || u > 1 || v < 0 || v > 1) return false;
    if (!this.hitMask) return true;

    const cx = Math.min(HIT_MASK_SIZE - 1, Math.max(0, Math.floor(u * HIT_MASK_SIZE)));
    const cy = Math.min(HIT_MASK_SIZE - 1, Math.max(0, Math.floor(v * HIT_MASK_SIZE)));

    // Check small 3-cell neighborhood for comfortable clicking around sprout/feet
    const pad = 2;
    for (let dy = -pad; dy <= pad; dy++) {
      const y = cy + dy;
      if (y < 0 || y >= HIT_MASK_SIZE) continue;
      for (let dx = -pad; dx <= pad; dx++) {
        const x = cx + dx;
        if (x < 0 || x >= HIT_MASK_SIZE) continue;
        if (this.hitMask[y * HIT_MASK_SIZE + x] > 16) {
          return true;
        }
      }
    }
    return false;
  }

  public dispose(): void {
    for (const list of this.frames.values()) {
      for (const bmp of list) {
        if (bmp && typeof ImageBitmap !== "undefined" && bmp instanceof ImageBitmap) {
          bmp.close();
        }
      }
    }
    this.frames.clear();
    this.fallbackFrame = null;
    this.hitMask = null;
  }
}

function buildHitMask(sprite: DrawableSprite): Uint8Array | null {
  try {
    const canvas = document.createElement("canvas");
    canvas.width = HIT_MASK_SIZE;
    canvas.height = HIT_MASK_SIZE;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return null;
    ctx.clearRect(0, 0, HIT_MASK_SIZE, HIT_MASK_SIZE);
    ctx.drawImage(sprite, 0, 0, HIT_MASK_SIZE, HIT_MASK_SIZE);
    const data = ctx.getImageData(0, 0, HIT_MASK_SIZE, HIT_MASK_SIZE).data;
    const mask = new Uint8Array(HIT_MASK_SIZE * HIT_MASK_SIZE);
    for (let i = 0; i < mask.length; i++) {
      mask[i] = data[i * 4 + 3];
    }
    return mask;
  } catch {
    return null;
  }
}

function loadHtmlImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = (e) => reject(e);
    img.src = url;
  });
}
