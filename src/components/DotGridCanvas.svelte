<script lang="ts">
import { onMount } from 'svelte';

let { active = false }: { active?: boolean } = $props();

let canvasEl = $state<HTMLCanvasElement | null>(null);
let isActiveRef = $derived(active);
let wakeUpFn: (() => void) | null = null;

$effect(() => {
  if (isActiveRef && wakeUpFn) {
    wakeUpFn();
  }
});

onMount(() => {
  const canvas = canvasEl;
  if (!canvas) return;
  const ctx = canvas.getContext('2d', { alpha: true });
  if (!ctx) return;

  const baseCanvas = document.createElement('canvas');
  const baseCtx = baseCanvas.getContext('2d', { alpha: true });

  let width = 0;
  let height = 0;
  let dpr = window.devicePixelRatio || 1;
  let mouseX = -9999;
  let mouseY = -9999;
  let smoothX = -9999;
  let smoothY = -9999;
  let activeEnergy = 0;
  let rafId = 0;
  let lastDrawTime = 0;
  let mounted = true;
  let pageVisible = typeof document !== 'undefined' ? !document.hidden : true;

  const SPACING = 24;
  const HALF_SPACING = SPACING / 2;
  const RADIUS = 135;
  const RADIUS_SQ = RADIUS * RADIUS;
  const ACTIVE_FPS_INTERVAL = 41; // ~24fps for ambient wave

  const COLOR_STEPS = 24;
  const colorLut: string[] = new Array(COLOR_STEPS + 1);
  for (let i = 0; i <= COLOR_STEPS; i++) {
    const t = i / COLOR_STEPS;
    const r = Math.round(255 - t * (255 - 77));
    const g = Math.round(255 - t * (255 - 107));
    const a = (0.032 + t * 0.065).toFixed(3);
    colorLut[i] = `rgba(${r}, ${g}, 255, ${a})`;
  }

  const buildBaseGrid = () => {
    if (!baseCtx || width <= 0 || height <= 0) return;
    baseCanvas.width = Math.max(1, Math.floor(width * dpr));
    baseCanvas.height = Math.max(1, Math.floor(height * dpr));
    baseCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
    baseCtx.clearRect(0, 0, width, height);
    const path = new Path2D();
    for (let x = HALF_SPACING; x < width; x += SPACING) {
      for (let y = HALF_SPACING; y < height; y += SPACING) {
        path.moveTo(x + 0.9, y);
        path.arc(x, y, 0.9, 0, Math.PI * 2);
      }
    }
    baseCtx.fillStyle = 'rgba(255, 255, 255, 0.032)';
    baseCtx.fill(path);
  };

  const renderFrame = (time: number) => {
    rafId = 0;
    if (!mounted || !pageVisible) return;

    const pointerInside = mouseX > -1000 && mouseY > -1000;
    const pointerSettling =
      pointerInside && (Math.abs(mouseX - smoothX) > 0.5 || Math.abs(mouseY - smoothY) > 0.5);

    if (!pointerSettling && (isActiveRef || activeEnergy > 0.004)) {
      if (time - lastDrawTime < ACTIVE_FPS_INTERVAL) {
        rafId = requestAnimationFrame(renderFrame);
        return;
      }
    }
    lastDrawTime = time;

    if (pointerInside) {
      smoothX += (mouseX - smoothX) * 0.18;
      smoothY += (mouseY - smoothY) * 0.18;
    } else {
      smoothX = -9999;
      smoothY = -9999;
    }
    activeEnergy += ((isActiveRef ? 1 : 0) - activeEnergy) * 0.08;
    if (!isActiveRef && activeEnergy < 0.004) {
      activeEnergy = 0;
    }

    ctx.clearRect(0, 0, width, height);

    if (activeEnergy <= 0.004 && !pointerInside) {
      if (baseCanvas.width > 0 && baseCanvas.height > 0) {
        ctx.drawImage(baseCanvas, 0, 0, width, height);
      }
      return;
    }

    if (activeEnergy <= 0.004 && pointerInside) {
      if (baseCanvas.width > 0 && baseCanvas.height > 0) {
        ctx.drawImage(baseCanvas, 0, 0, width, height);
      }
      const minX = Math.max(HALF_SPACING, Math.floor((smoothX - RADIUS) / SPACING) * SPACING + HALF_SPACING);
      const maxX = Math.min(width, Math.ceil((smoothX + RADIUS) / SPACING) * SPACING + HALF_SPACING);
      const minY = Math.max(HALF_SPACING, Math.floor((smoothY - RADIUS) / SPACING) * SPACING + HALF_SPACING);
      const maxY = Math.min(height, Math.ceil((smoothY + RADIUS) / SPACING) * SPACING + HALF_SPACING);

      for (let x = minX; x < maxX; x += SPACING) {
        for (let y = minY; y < maxY; y += SPACING) {
          const dx = x - smoothX;
          const dy = y - smoothY;
          const distSq = dx * dx + dy * dy;
          if (distSq < RADIUS_SQ) {
            const factor = 1 - Math.sqrt(distSq) / RADIUS;
            const eased = factor * factor;
            ctx.fillStyle = `rgba(255, 255, 255, ${(eased * 0.11).toFixed(3)})`;
            ctx.beginPath();
            ctx.arc(x, y, 0.9 + eased * 0.5, 0, Math.PI * 2);
            ctx.fill();
          }
        }
      }
    } else {
      const tPhase = time * 0.0032;
      for (let x = HALF_SPACING; x < width; x += SPACING) {
        for (let y = HALF_SPACING; y < height; y += SPACING) {
          const wave = Math.sin(x * 0.015 + y * 0.015 - tPhase) * 0.5 + 0.5;
          const blueMix = activeEnergy * wave;
          let radius = 0.9 + blueMix * 0.35;
          let extraAlpha = 0;

          if (pointerInside) {
            const dx = x - smoothX;
            const dy = y - smoothY;
            const distSq = dx * dx + dy * dy;
            if (distSq < RADIUS_SQ) {
              const factor = 1 - Math.sqrt(distSq) / RADIUS;
              const eased = factor * factor;
              extraAlpha = eased * 0.11;
              radius += eased * 0.5;
            }
          }

          if (extraAlpha > 0) {
            const r = Math.round(255 - blueMix * (255 - 77));
            const g = Math.round(255 - blueMix * (255 - 107));
            ctx.fillStyle = `rgba(${r}, ${g}, 255, ${(0.032 + blueMix * 0.065 + extraAlpha).toFixed(3)})`;
          } else {
            const idx = Math.min(COLOR_STEPS, Math.max(0, Math.round(blueMix * COLOR_STEPS)));
            ctx.fillStyle = colorLut[idx];
          }
          ctx.beginPath();
          ctx.arc(x, y, radius, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    const stillSettling =
      pointerInside && (Math.abs(mouseX - smoothX) > 0.5 || Math.abs(mouseY - smoothY) > 0.5);
    if (isActiveRef || activeEnergy > 0.004 || stillSettling) {
      rafId = requestAnimationFrame(renderFrame);
    }
  };

  const scheduleDraw = () => {
    if (!mounted || !pageVisible || rafId !== 0) return;
    rafId = requestAnimationFrame(renderFrame);
  };

  wakeUpFn = scheduleDraw;

  const resize = () => {
    const parent = canvas.parentElement;
    if (!parent) return;
    dpr = window.devicePixelRatio || 1;
    width = parent.clientWidth;
    height = parent.clientHeight;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    buildBaseGrid();
    scheduleDraw();
  };

  const onPointerMove = (e: PointerEvent) => {
    const rect = canvas.getBoundingClientRect();
    const nx = e.clientX - rect.left;
    const ny = e.clientY - rect.top;
    if (nx >= -RADIUS && nx <= width + RADIUS && ny >= -RADIUS && ny <= height + RADIUS) {
      if (smoothX < -1000) {
        smoothX = nx;
        smoothY = ny;
      }
      mouseX = nx;
      mouseY = ny;
      scheduleDraw();
    } else if (mouseX > -1000) {
      mouseX = -9999;
      mouseY = -9999;
      scheduleDraw();
    }
  };

  const onPointerLeave = () => {
    if (mouseX > -1000) {
      mouseX = -9999;
      mouseY = -9999;
      scheduleDraw();
    }
  };

  const onVisibilityChange = () => {
    pageVisible = !document.hidden;
    if (pageVisible) {
      scheduleDraw();
    } else if (rafId !== 0) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
  };

  resize();
  const ro = new ResizeObserver(resize);
  if (canvas.parentElement) ro.observe(canvas.parentElement);

  window.addEventListener('pointermove', onPointerMove, { passive: true });
  window.addEventListener('pointerleave', onPointerLeave);
  document.addEventListener('visibilitychange', onVisibilityChange);

  return () => {
    mounted = false;
    wakeUpFn = null;
    if (rafId !== 0) cancelAnimationFrame(rafId);
    ro.disconnect();
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerleave', onPointerLeave);
    document.removeEventListener('visibilitychange', onVisibilityChange);
  };
});
</script>

<canvas
  bind:this={canvasEl}
  class="absolute inset-0 w-full h-full pointer-events-none z-0"
></canvas>
