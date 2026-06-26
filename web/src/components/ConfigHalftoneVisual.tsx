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
      <div className="config-visual-meta">
        <div className="config-visual-kicker">
          <span className="status-dot ok" />
          <span>Rust research scouting system</span>
        </div>
        <h2>LITSCOUT-RS</h2>
        <p>把检索计划、结构化证据、引用审计与论文精读串成一条可检查的研究链路。</p>
        <div className="config-visual-facts">
          <span>RUNBOOK / STAGE 5.2</span>
          <span>RUST 2021</span>
          <span>TRACE SAFE</span>
        </div>
      </div>
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

  const drift = reduceMotion ? 0 : Math.sin(time * 0.32) * 1.8;
  const grid = width < 520 ? 48 : 64;
  context.save();
  context.lineWidth = 1;
  context.strokeStyle = colors.accentHover;
  context.globalAlpha = 0.055;
  for (let x = grid; x < width; x += grid) {
    context.beginPath();
    context.moveTo(x + drift, 0);
    context.lineTo(x + drift, height);
    context.stroke();
  }
  for (let y = grid; y < height; y += grid) {
    context.beginPath();
    context.moveTo(0, y - drift);
    context.lineTo(width, y - drift);
    context.stroke();
  }

  const step = width < 520 ? 28 : 34;
  const maxRadius = 1.8;
  const leftSource = { x: width * 0.18, y: height * 0.22, r: Math.min(width, height) * 0.42 };
  const rightSource = { x: width * 0.76, y: height * 0.72, r: Math.min(width, height) * 0.36 };

  context.fillStyle = colors.accent;
  for (let y = step * 0.5; y < height; y += step) {
    for (let x = step * 0.5; x < width; x += step) {
      const light =
        sourceFalloff(x, y, leftSource.x, leftSource.y, leftSource.r) * 0.65 +
        sourceFalloff(x, y, rightSource.x, rightSource.y, rightSource.r) * 0.45;
      const radius = 0.55 + light * maxRadius;
      context.globalAlpha = 0.05 + light * 0.1;
      context.beginPath();
      context.arc(x + drift * 0.4, y - drift * 0.3, radius, 0, Math.PI * 2);
      context.fill();
    }
  }
  context.globalAlpha = 0.09;
  context.fillStyle = colors.railAccent;
  context.beginPath();
  context.arc(width * 0.12, height * 0.16, 4, 0, Math.PI * 2);
  context.fill();
  context.restore();
}

export default ConfigHalftoneVisual;
