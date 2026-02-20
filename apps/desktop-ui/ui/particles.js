import * as THREE from 'three';
import { EffectComposer } from 'three/addons/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/addons/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/addons/postprocessing/UnrealBloomPass.js';

export class ParticleSystem {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        this.scene = new THREE.Scene();
        this.camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.1, 1000);
        this.camera.position.z = 55;

        this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
        this.renderer.setSize(window.innerWidth, window.innerHeight);
        this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        this.container.appendChild(this.renderer.domElement);

        this.composer = new EffectComposer(this.renderer);
        this.composer.addPass(new RenderPass(this.scene, this.camera));

        this.bloomPass = new UnrealBloomPass(new THREE.Vector2(window.innerWidth, window.innerHeight), 1.0, 0.4, 0.85);
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
                depthWrite: false
            })
        );
        this.scene.add(this.particles);

        this.currentState = 'neutral';
        this.shakeIntensity = 0;

        window.addEventListener('resize', this.onWindowResize.bind(this));

        this.setParticleState('neutral');
        this.animate();
    }

    onWindowResize() {
        this.camera.aspect = window.innerWidth / window.innerHeight;
        this.camera.updateProjectionMatrix();
        this.renderer.setSize(window.innerWidth, window.innerHeight);
        this.composer.setSize(window.innerWidth, window.innerHeight);
    }

    formText(text) {
        if (!text) {
            this.setParticleState('neutral');
            return;
        }

        this.currentState = 'text';
        this.bloomPass.strength = 1.2;
        this.shakeIntensity = 0;

        // Hidden 2D canvas to render text and extract pixel data
        const canvas2d = document.createElement('canvas');
        const ctx = canvas2d.getContext('2d', { willReadFrequently: true });

        // Define canvas size based on text length to avoid cropping
        canvas2d.width = 500;
        canvas2d.height = 200;

        ctx.fillStyle = '#000000';
        ctx.fillRect(0, 0, canvas2d.width, canvas2d.height);

        ctx.font = 'bold 60px "IBM Plex Mono", monospace';
        ctx.fillStyle = '#ffffff';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(text, canvas2d.width / 2, canvas2d.height / 2);

        const imgData = ctx.getImageData(0, 0, canvas2d.width, canvas2d.height).data;
        const validPixels = [];

        // Sample pixels to form target coordinates (invert Y axis for 3D)
        for (let y = 0; y < canvas2d.height; y += 2) {
            for (let x = 0; x < canvas2d.width; x += 2) {
                const index = (y * canvas2d.width + x) * 4;
                const r = imgData[index];
                if (r > 128) {
                    validPixels.push({
                        x: (x - canvas2d.width / 2) * 0.25,
                        y: -(y - canvas2d.height / 2) * 0.25,
                        z: (Math.random() - 0.5) * 2 // slight depth
                    });
                }
            }
        }

        // Map valid pixels to particles. If there are fewer pixels than particles, the rest become noise
        for (let i = 0; i < this.COUNT; i++) {
            if (i < validPixels.length) {
                const px = validPixels[i];
                this.targetPositions[i * 3] = px.x;
                this.targetPositions[i * 3 + 1] = px.y;
                this.targetPositions[i * 3 + 2] = px.z;

                // Bright cyan/white color for text
                this.targetColors[i * 3] = 0.5 + Math.random() * 0.5;
                this.targetColors[i * 3 + 1] = 0.8 + Math.random() * 0.2;
                this.targetColors[i * 3 + 2] = 1.0;
                this.targetSizes[i] = 1.2;
            } else {
                // Background noise
                const radius = 40 + Math.random() * 60;
                const theta = Math.random() * Math.PI * 2;
                const phi = Math.acos(2 * Math.random() - 1);
                this.targetPositions[i * 3] = radius * Math.sin(phi) * Math.cos(theta);
                this.targetPositions[i * 3 + 1] = radius * Math.sin(phi) * Math.sin(theta);
                this.targetPositions[i * 3 + 2] = radius * Math.cos(phi);

                this.targetColors[i * 3] = 0.1;
                this.targetColors[i * 3 + 1] = 0.1;
                this.targetColors[i * 3 + 2] = 0.2;
                this.targetSizes[i] = 0.2;
            }
        }
    }

    setParticleState(state) {
        if (this.currentState === state) return;
        this.currentState = state;

        this.shakeIntensity = 0;
        this.bloomPass.strength = 1.0;

        for (let i = 0; i < this.COUNT; i++) {
            if (state === 'neutral') {
                if (i < this.COUNT * 0.05) {
                    const r = 15 + Math.random() * 20;
                    const t = Math.random() * 6.28;
                    const ph = Math.random() * 3.14;
                    this.targetPositions[i * 3] = r * Math.sin(ph) * Math.cos(t);
                    this.targetPositions[i * 3 + 1] = r * Math.sin(ph) * Math.sin(t);
                    this.targetPositions[i * 3 + 2] = r * Math.cos(ph);

                    this.targetColors[i * 3] = 0.1;
                    this.targetColors[i * 3 + 1] = 0.1;
                    this.targetColors[i * 3 + 2] = 0.3;
                    this.targetSizes[i] = 0.4;
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

        for (let i = 0; i < this.COUNT * 3; i++) {
            pos[i] += (this.targetPositions[i] - pos[i]) * 0.1;
            col[i] += (this.targetColors[i] - col[i]) * 0.1;
        }
        for (let i = 0; i < this.COUNT; i++) {
            siz[i] += (this.targetSizes[i] - siz[i]) * 0.1;
        }

        this.particles.geometry.attributes.position.needsUpdate = true;
        this.particles.geometry.attributes.color.needsUpdate = true;
        this.particles.geometry.attributes.size.needsUpdate = true;

        if (this.currentState === 'text') {
            // Gentle floating for text
            this.particles.rotation.y = Math.sin(Date.now() * 0.001) * 0.1;
            this.particles.rotation.x = Math.cos(Date.now() * 0.001) * 0.05;
        } else {
            this.particles.rotation.y += 0.002;
        }

        this.composer.render();
    }
}
