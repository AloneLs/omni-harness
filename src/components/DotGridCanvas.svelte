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
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  let width = 0;
  let height = 0;
  let dpr = window.devicePixelRatio || 1;
  let mouseX = -9999;
  let mouseY = -9999;
  let smoothX = -9999;
  let smoothY = -9999;
  let activeEnergy = 0;
  let rafId = 0;
  let mounted = true;

  const SPACING = 24;
  const RADIUS = 135;

  const renderFrame = (time: number) => {
    rafId = 0;
    if (!mounted) return;

    const pointerInside = mouseX > -1000 && mouseY > -1000;
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
    const baseAlpha = 0.032;

    for (let x = SPACING / 2; x < width; x += SPACING) {
      for (let y = SPACING / 2; y < height; y += SPACING) {
        let alpha = baseAlpha;
        let radius = 0.9;
        let blueMix = 0;

        if (activeEnergy > 0.004) {
          const wave = Math.sin(x * 0.015 + y * 0.015 - time * 0.0032) * 0.5 + 0.5;
          alpha += activeEnergy * wave * 0.065;
          radius += activeEnergy * wave * 0.35;
          blueMix = activeEnergy * wave;
        }

        if (pointerInside) {
          const dx = x - smoothX;
          const dy = y - smoothY;
          const distSq = dx * dx + dy * dy;
          if (distSq < RADIUS * RADIUS) {
            const factor = 1 - Math.sqrt(distSq) / RADIUS;
            const eased = factor * factor;
            alpha += eased * 0.11;
            radius += eased * 0.5;
          }
        }

        if (blueMix > 0.005) {
          const r = Math.round(255 - blueMix * (255 - 77));
          const g = Math.round(255 - blueMix * (255 - 107));
          ctx.fillStyle = `rgba(${r}, ${g}, 255, ${alpha.toFixed(3)})`;
        } else {
          ctx.fillStyle = `rgba(255, 255, 255, ${alpha.toFixed(3)})`;
        }
        ctx.beginPath();
        ctx.arc(x, y, radius, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    const pointerSettling =
      pointerInside && (Math.abs(mouseX - smoothX) > 0.5 || Math.abs(mouseY - smoothY) > 0.5);
    if (isActiveRef || activeEnergy > 0.004 || pointerSettling) {
      rafId = requestAnimationFrame(renderFrame);
    }
  };

  const scheduleDraw = () => {
    if (!mounted || rafId !== 0) return;
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

  resize();
  const ro = new ResizeObserver(resize);
  if (canvas.parentElement) ro.observe(canvas.parentElement);

  window.addEventListener('pointermove', onPointerMove, { passive: true });
  window.addEventListener('pointerleave', onPointerLeave);

  return () => {
    mounted = false;
    wakeUpFn = null;
    if (rafId !== 0) cancelAnimationFrame(rafId);
    ro.disconnect();
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerleave', onPointerLeave);
  };
});
</script>

<canvas
  bind:this={canvasEl}
  class="absolute inset-0 w-full h-full pointer-events-none z-0"
></canvas>
