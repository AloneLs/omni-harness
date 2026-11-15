<script lang="ts">
import { onMount } from 'svelte';

let { active = false }: { active?: boolean } = $props();

let canvasEl = $state<HTMLCanvasElement | null>(null);
let isActiveRef = $derived(active);

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

  const SPACING = 20;
  const RADIUS = 135;

  const resize = () => {
    const parent = canvas.parentElement;
    if (!parent) return;
    dpr = window.devicePixelRatio || 1;
    width = parent.clientWidth;
    height = parent.clientHeight;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  };

  const onPointerMove = (e: PointerEvent) => {
    const rect = canvas.getBoundingClientRect();
    mouseX = e.clientX - rect.left;
    mouseY = e.clientY - rect.top;
  };

  const onPointerLeave = () => {
    mouseX = -9999;
    mouseY = -9999;
  };

  const draw = (time: number) => {
    if (!mounted) return;

    smoothX += (mouseX - smoothX) * 0.14;
    smoothY += (mouseY - smoothY) * 0.14;
    activeEnergy += ((isActiveRef ? 1 : 0) - activeEnergy) * 0.06;

    ctx.clearRect(0, 0, width, height);

    const pulse = Math.sin(time * 0.0012) * 0.005;
    const baseAlpha = 0.032 + pulse;

    for (let x = SPACING / 2; x < width; x += SPACING) {
      for (let y = SPACING / 2; y < height; y += SPACING) {
        let alpha = baseAlpha;
        let radius = 0.9;
        let blueMix = 0;

        if (activeEnergy > 0.01) {
          const wave = Math.sin(x * 0.015 + y * 0.015 - time * 0.0032) * 0.5 + 0.5;
          alpha += activeEnergy * wave * 0.065;
          radius += activeEnergy * wave * 0.35;
          blueMix = activeEnergy * wave;
        }

        const dx = x - smoothX;
        const dy = y - smoothY;
        const distSq = dx * dx + dy * dy;

        if (distSq < RADIUS * RADIUS) {
          const factor = 1 - Math.sqrt(distSq) / RADIUS;
          const eased = factor * factor;
          alpha += eased * 0.11;
          radius += eased * 0.5;
        }

        const r = Math.round(255 - blueMix * (255 - 77));
        const g = Math.round(255 - blueMix * (255 - 107));
        const b = 255;

        ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${alpha.toFixed(3)})`;
        ctx.beginPath();
        ctx.arc(x, y, radius, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    rafId = requestAnimationFrame(draw);
  };

  resize();
  const ro = new ResizeObserver(resize);
  if (canvas.parentElement) ro.observe(canvas.parentElement);

  window.addEventListener('pointermove', onPointerMove, { passive: true });
  window.addEventListener('pointerleave', onPointerLeave);
  rafId = requestAnimationFrame(draw);

  return () => {
    mounted = false;
    cancelAnimationFrame(rafId);
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
