import { useEffect, useRef } from "react";

interface CanvasSize {
  width: number;
  height: number;
  dpr: number;
}

interface HalftoneColors {
  accent: string;
  accentHover: string;
  railAccent: string;
  panel: string;
}

function ConfigHalftoneVisual() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvasElement = canvasRef.current;
    if (!canvasElement) return;
    const parentElement = canvasElement.parentElement;
    const canvasContext = canvasElement.getContext("2d");
    if (!parentElement || !canvasContext) return;

    const canvas: HTMLCanvasElement = canvasElement;
    const host: HTMLElement = parentElement;
    const context: CanvasRenderingContext2D = canvasContext;

    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animationFrame = 0;
    const size: CanvasSize = { width: 0, height: 0, dpr: 1 };
    const colors = readThemeColors();

    function resize() {
      resizeCanvas(canvas, host, context, size);
    }

    function draw(timestamp: number) {
      drawHalftone(context, size, colors, reduceMotion.matches, timestamp);

      if (!reduceMotion.matches) {
        animationFrame = window.requestAnimationFrame(draw);
      }
    }

    resize();
    draw(0);

    const observer = new ResizeObserver(() => {
      resize();
      draw(0);
    });
    observer.observe(host);

    const handleMotionChange = () => {
      window.cancelAnimationFrame(animationFrame);
      draw(0);
    };
    reduceMotion.addEventListener("change", handleMotionChange);

    return () => {
      observer.disconnect();
      reduceMotion.removeEventListener("change", handleMotionChange);
      window.cancelAnimationFrame(animationFrame);
    };
  }, []);

  return (
    <div className="config-halftone-visual" aria-hidden="true">
      <canvas ref={canvasRef} />
      <div className="config-halftone-letter">R</div>
      <div className="config-halftone-vignette" />
    </div>
  );
}

function readThemeColors(): HalftoneColors {
  const styles = window.getComputedStyle(document.documentElement);
  const readColor = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  return {
    accent: readColor("--accent", "#2a4bcc"),
    accentHover: readColor("--accent-hover", "#1e38a8"),
    railAccent: readColor("--rail-accent", "#4f6fef"),
    panel: readColor("--panel", "#fefdfa")
  };
}

function resizeCanvas(
  canvas: HTMLCanvasElement,
  host: HTMLElement,
  context: CanvasRenderingContext2D,
  size: CanvasSize
) {
  const rect = host.getBoundingClientRect();
  size.width = Math.max(1, Math.floor(rect.width));
  size.height = Math.max(1, Math.floor(rect.height));
  size.dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.floor(size.width * size.dpr);
  canvas.height = Math.floor(size.height * size.dpr);
  canvas.style.width = `${size.width}px`;
  canvas.style.height = `${size.height}px`;
  context.setTransform(size.dpr, 0, 0, size.dpr, 0, 0);
}

function sourceFalloff(x: number, y: number, cx: number, cy: number, radius: number) {
  const dx = x - cx;
  const dy = y - cy;
  const distance = Math.sqrt(dx * dx + dy * dy) / radius;
  return Math.max(0, 1 - distance);
}

function drawHalftone(
  context: CanvasRenderingContext2D,
  size: CanvasSize,
  colors: HalftoneColors,
  reduceMotion: boolean,
  timestamp: number
) {
  const { width, height } = size;
  const time = timestamp / 1000;
  context.clearRect(0, 0, width, height);

  const gradient = context.createLinearGradient(0, 0, width, height);
  gradient.addColorStop(0, colors.railAccent);
  gradient.addColorStop(0.44, colors.accent);
  gradient.addColorStop(1, colors.accentHover);
  context.fillStyle = gradient;
  context.fillRect(0, 0, width, height);

  const step = width < 520 ? 17 : 20;
  const maxRadius = step * 0.43;
  const drift = reduceMotion ? 0 : time * 0.55;
  const leftSource = { x: width * 0.26, y: height * 0.19, r: Math.min(width, height) * 0.36 };
  const rightSource = { x: width * 0.72, y: height * 0.66, r: Math.min(width, height) * 0.34 };
  const lowerSource = { x: width * 0.12, y: height * 0.92, r: Math.min(width, height) * 0.28 };

  context.fillStyle = colors.panel;
  for (let y = -step; y < height + step; y += step) {
    for (let x = -step; x < width + step; x += step) {
      const waveA = Math.sin((x * 0.018) + (y * 0.012) + drift);
      const waveB = Math.cos((x * 0.012) - (y * 0.021) - drift * 0.75);
      const bend = Math.sin((y / Math.max(height, 1)) * Math.PI * 2 + drift) * 9;
      const localX = x + bend * waveB;
      const localY = y + waveA * 5;

      const light =
        sourceFalloff(localX, localY, leftSource.x, leftSource.y, leftSource.r) * 1.2 +
        sourceFalloff(localX, localY, rightSource.x, rightSource.y, rightSource.r) * 1.15 +
        sourceFalloff(localX, localY, lowerSource.x, lowerSource.y, lowerSource.r) * 0.8;
      const field = 0.23 + light + (waveA + waveB) * 0.12;
      const radius = Math.max(1.4, Math.min(maxRadius, field * maxRadius));

      context.beginPath();
      context.arc(localX, localY, radius, 0, Math.PI * 2);
      context.fill();
    }
  }
}

export default ConfigHalftoneVisual;
