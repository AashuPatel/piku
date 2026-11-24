import { useEffect, useRef } from "react";
import type { AnimationController } from "../../animation/controller";
import type { CharacterManifest, FrameSnapshot } from "../../animation/types";

interface PetCanvasProps {
  controller: AnimationController;
  manifest: CharacterManifest;
  size: number;
  showDevOverlay?: boolean;
  showShadow?: boolean;
  onPointerDown?: React.PointerEventHandler<HTMLCanvasElement>;
  onPointerMove?: React.PointerEventHandler<HTMLCanvasElement>;
  onPointerUp?: React.PointerEventHandler<HTMLCanvasElement>;
  onPointerCancel?: React.PointerEventHandler<HTMLCanvasElement>;
}

/**
 * Dynamic grounding-shadow scale factor for airborne animation frames
 * (happy frames 02..04 and click spring frame 04).
 */
function shadowScaleForFrame(snap: FrameSnapshot): { scaleX: number; alpha: number } {
  if (snap.state === "sleep") {
    return { scaleX: 1.24, alpha: 0.14 };
  }
  if (snap.state === "happy") {
    if (snap.frameIndex === 1) return { scaleX: 0.84, alpha: 0.1 };
    if (snap.frameIndex === 2) return { scaleX: 0.66, alpha: 0.075 };
    if (snap.frameIndex === 3) return { scaleX: 0.8, alpha: 0.095 };
    if (snap.frameIndex === 0 || snap.frameIndex === 4) return { scaleX: 1.08, alpha: 0.14 };
  }
  if (snap.state === "click") {
    if (snap.frameIndex === 1) return { scaleX: 1.12, alpha: 0.14 };
    if (snap.frameIndex === 2) return { scaleX: 1.2, alpha: 0.15 };
    if (snap.frameIndex === 3) return { scaleX: 0.74, alpha: 0.085 };
  }
  return { scaleX: 1.0, alpha: 0.12 };
}

/**
 * Dirty-frame Canvas sprite renderer.
 * Zero React rerenders per animation frame: subscribes imperatively to AnimationController.
 */
export function PetCanvas({
  controller,
  manifest,
  size,
  showDevOverlay = false,
  showShadow = true,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onPointerCancel,
}: PetCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const overlayRef = useRef(showDevOverlay);
  const shadowRef = useRef(showShadow);
  overlayRef.current = showDevOverlay;
  shadowRef.current = showShadow;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    let latestSnapshot = controller.getSnapshot();

    const draw = (snap: FrameSnapshot) => {
      latestSnapshot = snap;
      const dpr = window.devicePixelRatio || 1;
      const px = Math.round(size * dpr);

      if (canvas.width !== px || canvas.height !== px) {
        canvas.width = px;
        canvas.height = px;
      }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, size, size);

      const anchorX = size * manifest.anchor.x;
      const anchorY = size * manifest.anchor.y;

      // 1. Subtle dynamic grounding shadow (never baked into PNGs)
      if (shadowRef.current) {
        const { scaleX, alpha } = shadowScaleForFrame(snap);
        const rx = size * 0.2 * scaleX;
        const ry = size * 0.038;
        ctx.save();
        ctx.fillStyle = `rgba(80, 75, 60, ${alpha})`;
        ctx.beginPath();
        ctx.ellipse(anchorX, anchorY + size * 0.008, rx, ry, 0, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }

      // 2. Active sprite frame (256x256 ImageBitmap drawn cleanly at logical size)
      if (snap.bitmap) {
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(snap.bitmap, 0, 0, size, size);
      }

      // 3. Optional Dev Anchor & Safe-Margin Overlay (Ctrl+Shift+G)
      if (overlayRef.current) {
        ctx.save();
        // Safe border (4px on 256 -> scaled proportionally)
        const margin = (4 / manifest.frameSize.width) * size;
        ctx.strokeStyle = "rgba(84, 140, 210, 0.7)";
        ctx.lineWidth = 1;
        ctx.strokeRect(margin, margin, size - margin * 2, size - margin * 2);

        // Canonical vertical centerline & ground baseline
        ctx.strokeStyle = "rgba(210, 72, 72, 0.75)";
        ctx.beginPath();
        ctx.moveTo(anchorX, 0);
        ctx.lineTo(anchorX, size);
        ctx.moveTo(0, anchorY);
        ctx.lineTo(size, anchorY);
        ctx.stroke();

        // Compact state/frame debug label
        const label = `${snap.state} ${String(snap.frameIndex + 1).padStart(2, "0")}/${String(
          snap.frameCount
        ).padStart(2, "0")} · ${snap.fps}fps`;
        ctx.font = '700 10px "Nunito", sans-serif';
        const tw = ctx.measureText(label).width;
        ctx.fillStyle = "rgba(69, 69, 61, 0.82)";
        ctx.beginPath();
        ctx.roundRect(4, 4, tw + 10, 16, 6);
        ctx.fill();
        ctx.fillStyle = "#FFFEFA";
        ctx.fillText(label, 9, 15);
        ctx.restore();
      }
    };

    const unsubscribe = controller.onFrameChange(draw);

    const handleResize = () => draw(latestSnapshot);
    window.addEventListener("resize", handleResize);

    return () => {
      unsubscribe();
      window.removeEventListener("resize", handleResize);
    };
  }, [controller, manifest, size, showDevOverlay, showShadow]);

  return (
    <canvas
      ref={canvasRef}
      width={size}
      height={size}
      style={{ width: size, height: size, display: "block" }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onContextMenu={(e) => e.preventDefault()}
    />
  );
}
