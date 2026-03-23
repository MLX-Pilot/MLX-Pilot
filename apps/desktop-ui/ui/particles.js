import * as THREE from 'three';
import { EffectComposer } from 'three/addons/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/addons/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/addons/postprocessing/UnrealBloomPass.js';

export class ParticleSystem {
  constructor(containerId) {
    this.container = document.getElementById(containerId);
    if (!this.container) {
      throw new Error(`Container '${containerId}' nao encontrado para o sistema de particulas.`);
    }

    const { width, height } = this.getViewportSize();
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(75, width / height, 0.1, 1000);
    this.camera.position.z = 55;

    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setSize(width, height);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.container.appendChild(this.renderer.domElement);

    this.composer = new EffectComposer(this.renderer);
    this.composer.addPass(new RenderPass(this.scene, this.camera));

    this.bloomPass = new UnrealBloomPass(new THREE.Vector2(width, height), 1.0, 0.4, 0.85);
    this.composer.addPass(this.bloomPass);

    this.COUNT = 20000;
    this.geometry = new THREE.BufferGeometry();
    this.positions = new Float32Array(this.COUNT * 3);
    this.colors = new Float32Array(this.COUNT * 3);
    this.sizes = new Float32Array(this.COUNT);

    this.targetPositions = new Float32Array(this.COUNT * 3);
    this.targetColors = new Float32Array(this.COUNT * 3);
    this.targetSizes = new Float32Array(this.COUNT);

    this.geometry.setAttribute('position', new THREE.BufferAttribute(this.positions, 3));
    this.geometry.setAttribute('color', new THREE.BufferAttribute(this.colors, 3));
    this.geometry.setAttribute('size', new THREE.BufferAttribute(this.sizes, 1));

    this.particles = new THREE.Points(
      this.geometry,
      new THREE.PointsMaterial({
        size: 0.3,
        vertexColors: true,
        blending: THREE.AdditiveBlending,
        transparent: true,
        depthWrite: false,
      })
    );
    this.scene.add(this.particles);

    this.currentState = 'neutral';
    this.shakeIntensity = 0;

    this.scriptFrames = [];
    this.scriptFrameIndex = -1;
    this.scriptTimer = null;
    this.scriptLoop = false;
    this.scriptCallbacks = {
      onFrame: null,
      onComplete: null,
    };

    this.sceneCanvas = document.createElement('canvas');
    this.sceneCanvas.width = 960;
    this.sceneCanvas.height = 540;
    this.sceneCtx = this.sceneCanvas.getContext('2d', { willReadFrequently: true });

    this.textCanvas = document.createElement('canvas');
    this.textCanvas.width = 600;
    this.textCanvas.height = 220;
    this.textCtx = this.textCanvas.getContext('2d', { willReadFrequently: true });

    window.addEventListener('resize', this.onWindowResize.bind(this));

    this.setParticleState('neutral');
    this.animate();
  }

  getViewportSize() {
    const width = Math.max(this.container.clientWidth || 0, window.innerWidth || 1);
    const height = Math.max(this.container.clientHeight || 0, window.innerHeight || 1);
    return { width, height };
  }

  onWindowResize() {
    const { width, height } = this.getViewportSize();
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(width, height);
    this.composer.setSize(width, height);
  }

  stopScene() {
    this.stopScriptPlayback();
    if (this.currentState === 'script') {
      this.setParticleState('neutral');
    }
  }

  playScript(script, options = {}) {
    const frames = Array.isArray(script?.frames)
      ? script.frames.filter((frame) => frame && typeof frame === 'object')
      : [];

    if (!frames.length) {
      this.setParticleState('neutral');
      return;
    }

    this.stopScriptPlayback();
    this.scriptFrames = frames;
    this.scriptFrameIndex = -1;
    this.scriptLoop = Boolean(options.loop);
    this.scriptCallbacks = {
      onFrame: typeof options.onFrame === 'function' ? options.onFrame : null,
      onComplete: typeof options.onComplete === 'function' ? options.onComplete : null,
    };

    this.currentState = 'script';
    this.bloomPass.strength = 1.35;
    this.advanceScriptFrame();
  }

  stopScriptPlayback() {
    if (this.scriptTimer) {
      window.clearTimeout(this.scriptTimer);
      this.scriptTimer = null;
    }
    this.scriptFrameIndex = -1;
    this.scriptFrames = [];
    this.scriptLoop = false;
    this.scriptCallbacks = {
      onFrame: null,
      onComplete: null,
    };
  }

  advanceScriptFrame() {
    if (!this.scriptFrames.length) {
      return;
    }

    this.scriptFrameIndex += 1;
    if (this.scriptFrameIndex >= this.scriptFrames.length) {
      if (!this.scriptLoop) {
        this.scriptFrameIndex = this.scriptFrames.length - 1;
        const onComplete = this.scriptCallbacks.onComplete;
        if (onComplete) {
          onComplete();
        }
        this.scriptTimer = null;
        return;
      }
      this.scriptFrameIndex = 0;
    }

    const frame = this.scriptFrames[this.scriptFrameIndex];
    this.renderSceneFrame(frame);

    const onFrame = this.scriptCallbacks.onFrame;
    if (onFrame) {
      onFrame(this.scriptFrameIndex, frame);
    }

    const duration = this.clampNumber(frame.duration_ms, 900, 9000, 2200);
    this.scriptTimer = window.setTimeout(() => {
      this.advanceScriptFrame();
    }, duration);
  }

  formText(text) {
    if (!text) {
      this.setParticleState('neutral');
      return;
    }

    this.stopScriptPlayback();
    this.currentState = 'text';
    this.bloomPass.strength = 1.2;
    this.shakeIntensity = 0;

    const ctx = this.textCtx;
    const canvas = this.textCanvas;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.fillStyle = '#ffffff';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.font = '700 66px "IBM Plex Mono", monospace';
    ctx.fillText(String(text), canvas.width / 2, canvas.height / 2);

    this.applyCanvasToTargets(canvas, 2);
  }

  renderSceneFrame(frame) {
    const ctx = this.sceneCtx;
    const canvas = this.sceneCanvas;
    const width = canvas.width;
    const height = canvas.height;

    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = '#02030b';
    ctx.fillRect(0, 0, width, height);

    this.drawBackground(frame, ctx, width, height);

    const shapes = Array.isArray(frame.shapes) ? frame.shapes : [];
    for (const shape of shapes) {
      this.drawShape(shape, ctx, width, height);
    }

    this.applyCanvasToTargets(canvas, 3);
  }

  drawBackground(frame, ctx, width, height) {
    const background = frame.background || {};
    const baseColor = typeof background.color === 'string' ? background.color : '#050816';
    const glowColor = typeof background.glow === 'string' ? background.glow : '#1f3f7d';

    ctx.fillStyle = baseColor;
    ctx.globalAlpha = 0.45;
    ctx.fillRect(0, 0, width, height);

    const gradient = ctx.createRadialGradient(width * 0.5, height * 0.5, width * 0.08, width * 0.5, height * 0.5, width * 0.55);
    gradient.addColorStop(0, `${glowColor}66`);
    gradient.addColorStop(1, '#00000000');

    ctx.globalAlpha = 0.85;
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, width, height);

    ctx.globalAlpha = 1;
  }

  drawShape(shape, ctx, width, height) {
    const type = String(shape?.type || '').trim().toLowerCase();
    switch (type) {
      case 'text':
        this.drawTextShape(shape, ctx, width, height);
        break;
      case 'line':
        this.drawLineShape(shape, ctx, width, height);
        break;
      case 'arrow':
        this.drawArrowShape(shape, ctx, width, height);
        break;
      case 'circle':
      case 'ring':
        this.drawCircleShape(shape, ctx, width, height, type === 'ring');
        break;
      case 'rect':
      case 'box':
        this.drawRectShape(shape, ctx, width, height);
        break;
      case 'spiral':
        this.drawSpiralShape(shape, ctx, width, height);
        break;
      case 'wave':
        this.drawWaveShape(shape, ctx, width, height);
        break;
      case 'point':
      case 'dot':
        this.drawDotShape(shape, ctx, width, height);
        break;
      default:
        break;
    }
  }

  drawTextShape(shape, ctx, width, height) {
    const x = this.percentToX(shape.x, width, 50);
    const y = this.percentToY(shape.y, height, 50);
    const size = this.clampNumber(shape.size, 10, 110, 34);
    const weight = this.clampNumber(shape.weight, 300, 900, 700);
    const color = typeof shape.color === 'string' ? shape.color : '#c6e8ff';
    const text = String(shape.text || '').trim();
    const alignRaw = String(shape.align || 'center').toLowerCase();
    const align = ['left', 'center', 'right'].includes(alignRaw) ? alignRaw : 'center';

    if (!text) {
      return;
    }

    ctx.save();
    ctx.textAlign = align;
    ctx.textBaseline = 'middle';
    ctx.fillStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = Math.max(6, size * 0.32);
    ctx.font = `${weight} ${size}px "IBM Plex Mono", monospace`;
    ctx.fillText(text, x, y);
    ctx.restore();
  }

  drawLineShape(shape, ctx, width, height) {
    const x1 = this.percentToX(shape.x1, width, 30);
    const y1 = this.percentToY(shape.y1, height, 30);
    const x2 = this.percentToX(shape.x2, width, 70);
    const y2 = this.percentToY(shape.y2, height, 70);
    const lineWidth = this.clampNumber(shape.width, 1, 16, 4);
    const color = typeof shape.color === 'string' ? shape.color : '#5ec7ff';

    ctx.save();
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.strokeStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = lineWidth * 1.8;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
    ctx.restore();
  }

  drawArrowShape(shape, ctx, width, height) {
    const x1 = this.percentToX(shape.x1, width, 30);
    const y1 = this.percentToY(shape.y1, height, 30);
    const x2 = this.percentToX(shape.x2, width, 70);
    const y2 = this.percentToY(shape.y2, height, 70);
    const lineWidth = this.clampNumber(shape.width, 1, 14, 4);
    const head = this.clampNumber(shape.head, 6, 34, 16);
    const color = typeof shape.color === 'string' ? shape.color : '#79dcff';

    const angle = Math.atan2(y2 - y1, x2 - x1);

    ctx.save();
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = lineWidth * 2;
    ctx.lineWidth = lineWidth;

    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(x2, y2);
    ctx.lineTo(x2 - head * Math.cos(angle - Math.PI / 6), y2 - head * Math.sin(angle - Math.PI / 6));
    ctx.lineTo(x2 - head * Math.cos(angle + Math.PI / 6), y2 - head * Math.sin(angle + Math.PI / 6));
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }

  drawCircleShape(shape, ctx, width, height, ringOnly) {
    const x = this.percentToX(shape.x, width, 50);
    const y = this.percentToY(shape.y, height, 50);
    const r = this.clampNumber(shape.r, 4, 320, 80);
    const lineWidth = this.clampNumber(shape.width, 1, 18, 5);
    const color = typeof shape.color === 'string' ? shape.color : '#5cc2ff';
    const fill = ringOnly ? null : (typeof shape.fill === 'string' ? shape.fill : `${color}22`);

    ctx.save();
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);

    if (fill) {
      ctx.fillStyle = fill;
      ctx.fill();
    }

    ctx.strokeStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = lineWidth * 2;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
    ctx.restore();
  }

  drawRectShape(shape, ctx, width, height) {
    const x = this.percentToX(shape.x, width, 50);
    const y = this.percentToY(shape.y, height, 50);
    const w = this.clampNumber(shape.w, 6, 640, 180);
    const h = this.clampNumber(shape.h, 6, 420, 100);
    const lineWidth = this.clampNumber(shape.width, 1, 16, 4);
    const color = typeof shape.color === 'string' ? shape.color : '#6bc9ff';
    const fill = typeof shape.fill === 'string' ? shape.fill : 'transparent';

    ctx.save();
    if (fill !== 'transparent') {
      ctx.fillStyle = fill;
      ctx.fillRect(x - w / 2, y - h / 2, w, h);
    }

    ctx.strokeStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = lineWidth * 1.8;
    ctx.lineWidth = lineWidth;
    ctx.strokeRect(x - w / 2, y - h / 2, w, h);
    ctx.restore();
  }

  drawSpiralShape(shape, ctx, width, height) {
    const x = this.percentToX(shape.x, width, 50);
    const y = this.percentToY(shape.y, height, 50);
    const radius = this.clampNumber(shape.r, 10, 340, 140);
    const turns = this.clampNumber(shape.turns, 1, 12, 4);
    const lineWidth = this.clampNumber(shape.width, 1, 12, 3);
    const color = typeof shape.color === 'string' ? shape.color : '#81dfff';

    const steps = 180;
    ctx.save();
    ctx.beginPath();

    for (let i = 0; i <= steps; i += 1) {
      const t = i / steps;
      const theta = turns * Math.PI * 2 * t;
      const rad = radius * t;
      const px = x + Math.cos(theta) * rad;
      const py = y + Math.sin(theta) * rad;
      if (i === 0) {
        ctx.moveTo(px, py);
      } else {
        ctx.lineTo(px, py);
      }
    }

    ctx.strokeStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = lineWidth * 2;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
    ctx.restore();
  }

  drawWaveShape(shape, ctx, width, height) {
    const x = this.percentToX(shape.x, width, 50);
    const y = this.percentToY(shape.y, height, 50);
    const length = this.clampNumber(shape.length, 20, 860, 360);
    const amp = this.clampNumber(shape.amp, 4, 120, 26);
    const cycles = this.clampNumber(shape.cycles, 1, 16, 3);
    const lineWidth = this.clampNumber(shape.width, 1, 10, 3);
    const color = typeof shape.color === 'string' ? shape.color : '#8ce4ff';

    const startX = x - length / 2;
    const steps = 160;

    ctx.save();
    ctx.beginPath();
    for (let i = 0; i <= steps; i += 1) {
      const t = i / steps;
      const px = startX + t * length;
      const py = y + Math.sin(t * cycles * Math.PI * 2) * amp;
      if (i === 0) {
        ctx.moveTo(px, py);
      } else {
        ctx.lineTo(px, py);
      }
    }

    ctx.strokeStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = lineWidth * 2;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
    ctx.restore();
  }

  drawDotShape(shape, ctx, width, height) {
    const x = this.percentToX(shape.x, width, 50);
    const y = this.percentToY(shape.y, height, 50);
    const r = this.clampNumber(shape.r, 1, 80, 8);
    const color = typeof shape.color === 'string' ? shape.color : '#9ce7ff';

    ctx.save();
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.shadowColor = color;
    ctx.shadowBlur = r * 2.5;
    ctx.fill();
    ctx.restore();
  }

  applyCanvasToTargets(canvas, sampleStep = 3) {
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    const image = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    const valid = [];

    for (let y = 0; y < canvas.height; y += sampleStep) {
      for (let x = 0; x < canvas.width; x += sampleStep) {
        const index = (y * canvas.width + x) * 4;
        const a = image[index + 3];
        if (a < 34) {
          continue;
        }
        valid.push({
          x,
          y,
          r: image[index],
          g: image[index + 1],
          b: image[index + 2],
          a,
        });
      }
    }

    const scale = 0.14;
    const halfW = canvas.width * 0.5;
    const halfH = canvas.height * 0.5;

    for (let i = 0; i < this.COUNT; i += 1) {
      if (valid.length && i < valid.length) {
        const sample = valid[(i * 37) % valid.length];
        const jitter = 0.18;

        this.targetPositions[i * 3] = (sample.x - halfW) * scale + (Math.random() - 0.5) * jitter;
        this.targetPositions[i * 3 + 1] = -(sample.y - halfH) * scale + (Math.random() - 0.5) * jitter;
        this.targetPositions[i * 3 + 2] = (Math.random() - 0.5) * 2.5;

        this.targetColors[i * 3] = sample.r / 255;
        this.targetColors[i * 3 + 1] = sample.g / 255;
        this.targetColors[i * 3 + 2] = sample.b / 255;

        this.targetSizes[i] = 0.9 + (sample.a / 255) * 0.9;
      } else {
        const radius = 32 + Math.random() * 48;
        const theta = Math.random() * Math.PI * 2;
        const phi = Math.acos(2 * Math.random() - 1);

        this.targetPositions[i * 3] = radius * Math.sin(phi) * Math.cos(theta);
        this.targetPositions[i * 3 + 1] = radius * Math.sin(phi) * Math.sin(theta);
        this.targetPositions[i * 3 + 2] = radius * Math.cos(phi);

        this.targetColors[i * 3] = 0.03;
        this.targetColors[i * 3 + 1] = 0.05;
        this.targetColors[i * 3 + 2] = 0.1;

        this.targetSizes[i] = 0.16;
      }
    }
  }

  setParticleState(state) {
    if (state !== 'script') {
      this.stopScriptPlayback();
    }

    if (this.currentState === state && state !== 'neutral') {
      return;
    }
    this.currentState = state;

    this.shakeIntensity = 0;
    this.bloomPass.strength = 1.0;

    for (let i = 0; i < this.COUNT; i += 1) {
      if (state === 'none') {
        this.targetPositions[i * 3] = 0;
        this.targetPositions[i * 3 + 1] = 0;
        this.targetPositions[i * 3 + 2] = 0;

        this.targetColors[i * 3] = 0;
        this.targetColors[i * 3 + 1] = 0;
        this.targetColors[i * 3 + 2] = 0;

        this.targetSizes[i] = 0;
        continue;
      }

      if (state === 'neutral') {
        if (i < this.COUNT * 0.08) {
          const radius = 10 + Math.random() * 26;
          const theta = Math.random() * Math.PI * 2;
          const phi = Math.random() * Math.PI;

          this.targetPositions[i * 3] = radius * Math.sin(phi) * Math.cos(theta);
          this.targetPositions[i * 3 + 1] = radius * Math.sin(phi) * Math.sin(theta);
          this.targetPositions[i * 3 + 2] = radius * Math.cos(phi);

          this.targetColors[i * 3] = 0.08;
          this.targetColors[i * 3 + 1] = 0.14;
          this.targetColors[i * 3 + 2] = 0.36;
          this.targetSizes[i] = 0.38;
        } else {
          this.targetPositions[i * 3] = 0;
          this.targetPositions[i * 3 + 1] = 0;
          this.targetPositions[i * 3 + 2] = 0;

          this.targetColors[i * 3] = 0;
          this.targetColors[i * 3 + 1] = 0;
          this.targetColors[i * 3 + 2] = 0;

          this.targetSizes[i] = 0;
        }
      }
    }
  }

  animate() {
    requestAnimationFrame(this.animate.bind(this));

    const pos = this.particles.geometry.attributes.position.array;
    const col = this.particles.geometry.attributes.color.array;
    const siz = this.particles.geometry.attributes.size.array;

    for (let i = 0; i < this.COUNT * 3; i += 1) {
      pos[i] += (this.targetPositions[i] - pos[i]) * 0.1;
      col[i] += (this.targetColors[i] - col[i]) * 0.1;
    }

    for (let i = 0; i < this.COUNT; i += 1) {
      siz[i] += (this.targetSizes[i] - siz[i]) * 0.1;
    }

    this.particles.geometry.attributes.position.needsUpdate = true;
    this.particles.geometry.attributes.color.needsUpdate = true;
    this.particles.geometry.attributes.size.needsUpdate = true;

    if (this.currentState === 'script') {
      this.particles.rotation.y = Math.sin(Date.now() * 0.00025) * 0.12;
      this.particles.rotation.x = Math.cos(Date.now() * 0.00019) * 0.08;
    } else if (this.currentState === 'text') {
      this.particles.rotation.y = Math.sin(Date.now() * 0.001) * 0.1;
      this.particles.rotation.x = Math.cos(Date.now() * 0.001) * 0.05;
    } else if (this.currentState === 'neutral') {
      this.particles.rotation.y += 0.0018;
      this.particles.rotation.x *= 0.98;
    } else {
      this.particles.rotation.y *= 0.95;
      this.particles.rotation.x *= 0.95;
    }

    this.composer.render();
  }

  percentToX(value, width, fallback) {
    const pct = this.clampNumber(value, 0, 100, fallback);
    return (pct / 100) * width;
  }

  percentToY(value, height, fallback) {
    const pct = this.clampNumber(value, 0, 100, fallback);
    return (pct / 100) * height;
  }

  clampNumber(value, min, max, fallback) {
    const number = Number(value);
    if (!Number.isFinite(number)) {
      return fallback;
    }
    return Math.max(min, Math.min(max, number));
  }
}
