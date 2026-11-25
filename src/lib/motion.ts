import { Spring } from 'svelte/motion';

export const SPRING_LAUNCH = { stiffness: 0.34, damping: 0.86 } as const;
export const SPRING_PANEL = { stiffness: 0.25, damping: 0.84 } as const;
export const SPRING_POPUP = { stiffness: 0.24, damping: 0.80 } as const;
export const SPRING_KEYCAP = { stiffness: 0.36, damping: 0.72 } as const;
export const SPRING_SLIDER = { stiffness: 0.32, damping: 0.78 } as const;

export function attachPressScale(node: HTMLElement, targetScale = 0.95) {
  const scaleSpring = new Spring(1, { stiffness: 0.28, damping: 0.76 });
  let rafActive = false;
  let disposed = false;

  const sync = () => {
    if (!rafActive || disposed) return;
    const cur = scaleSpring.current;
    node.style.transform = `scale(${cur})`;
    if (Math.abs(cur - scaleSpring.target) < 0.0005) {
      rafActive = false;
      node.style.transform = scaleSpring.target === 1 ? '' : `scale(${scaleSpring.target})`;
      return;
    }
    requestAnimationFrame(sync);
  };
  const wake = () => {
    if (rafActive || disposed) return;
    rafActive = true;
    requestAnimationFrame(sync);
  };

  const down = () => {
    scaleSpring.target = targetScale;
    wake();
  };
  const up = () => {
    scaleSpring.target = 1;
    wake();
  };

  node.addEventListener('pointerdown', down);
  node.addEventListener('pointerup', up);
  node.addEventListener('pointerleave', up);
  node.addEventListener('pointercancel', up);

  return {
    destroy() {
      disposed = true;
      rafActive = false;
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointerup', up);
      node.removeEventListener('pointerleave', up);
      node.removeEventListener('pointercancel', up);
      node.style.transform = '';
    }
  };
}

export function animateSwapIcon(
  node: HTMLElement,
  onSwap?: () => void,
  duration = 160
): Promise<void> {
  return new Promise((resolve) => {
    const half = duration / 2;
    const outAnim = node.animate(
      [
        { transform: 'scale(1)', opacity: 1 },
        { transform: 'scale(0.65)', opacity: 0 }
      ],
      { duration: half, easing: 'cubic-bezier(0, 0, 0.2, 1)', fill: 'forwards' }
    );
    outAnim.onfinish = () => {
      if (onSwap) onSwap();
      const inAnim = node.animate(
        [
          { transform: 'scale(0.65)', opacity: 0 },
          { transform: 'scale(1)', opacity: 1 }
        ],
        { duration: half, easing: 'cubic-bezier(0.34, 1.56, 0.64, 1)', fill: 'forwards' }
      );
      inAnim.onfinish = () => resolve();
    };
  });
}
