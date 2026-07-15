<template>
  <div class="about-container" ref="containerRef">
    <!-- Canvas 雪花层 -->
    <canvas ref="canvasRef" class="snow-canvas"></canvas>

    <!-- 极简的时间线背景线 -->
    <div class="static-glass background"></div>
    <div class="about-content">
      <!-- Logo 和版本号，带进入动画 -->
      <div class="app-header">
        <fv-img src="/phi-tklogo.png" alt="Phi TKC" class="fluent-glass app-logo-img" />
		<p class="fluent-glass version-badge">v{{appVersion}}</p>
      </div>
      <!-- 信息卡片，纵向排列，依次滑入 -->
      <div class="info-cards">
        <v-card class="info-card" :style="{ '--i': 0 }" @click="openGitHub" ripple>
          <div class="fluent-glass card-content">
            <div class="card-icon">
              <fv-AnimatedIcon fontSize="28" icon="Globe" />
            </div>
            <div class="card-text">
              <h3 class="card-title">GitHub</h3>
              <p class="card-subtitle">View source code</p>
            </div>
            <div class="card-link">
              <fv-AnimatedIcon fontSize="20" icon="Link" />
            </div>
          </div>
        </v-card>
        <v-card class="info-card" :style="{ '--i': 0 }" @click="openLicense" ripple>
          <div class="fluent-glass card-content">
            <div class="card-icon">
              <fv-AnimatedIcon fontSize="28" icon="PrintAllPages" />
            </div>
            <div class="card-text">
              <h3 class="card-title">License</h3>
              <p class="card-subtitle">{{ t('license') }}</p>
            </div>
            <div class="card-link">
              <fv-AnimatedIcon fontSize="20" icon="Link" />
            </div>
          </div>
        </v-card>
        <v-card class="info-card" :style="{ '--i': 2 }">
          <div class="fluent-glass card-content">
            <div class="card-icon">
              <fv-AnimatedIcon fontSize="28" icon="Package" />
            </div>
            <div class="card-text">
              <h3 class="card-title">Version</h3>
              <p class="card-subtitle">v{{ appVersion }}</p>
            </div>
          </div>
        </v-card>
      </div>
      <!-- 底部版权（传入动态年份） -->
      <div class="fluent-glass about-footer">
        		<p class="footer-sec">{{ t('footer.copyright', { year: currentYear }) }}</p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import { getVersion } from '@tauri-apps/api/app';
import { open } from '@tauri-apps/plugin-shell';

const { t } = useI18n();
const currentYear = ref(new Date().getFullYear());
const appVersion = ref('1.0.0');

// Canvas 引用
const containerRef = ref<HTMLElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);

let animationFrameId: number | null = null;
let particles: Particle[] = [];
let ctx: CanvasRenderingContext2D | null = null;

interface Particle {
  x: number;
  y: number;
  radius: number;
  speedY: number;
  speedX: number;
}

const fetchVersion = async () => {
  try {
    appVersion.value = await getVersion();
  } catch (e) {
    console.error('Failed to get version:', e);
  }
};

const openGitHub = () => {
  open('https://github.com/CMDBookDevelope/Phi-TKC').catch((e) => {
    console.error('Failed to open GitHub:', e);
  });
};

const openLicense = () => {
  open('https://github.com/CMDBookDevelope/Phi-TKC/blob/main/LICENSE').catch((e) => {
    console.error('Failed to open GitHub:', e);
  });
};

// 初始化 Canvas 和粒子
const initCanvas = () => {
  const container = containerRef.value;
  const canvas = canvasRef.value;
  if (!container || !canvas) return;

  const rect = container.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;

  // 设置实际尺寸（物理像素）
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  // 设置样式尺寸（CSS 像素）
  canvas.style.width = rect.width + 'px';
  canvas.style.height = rect.height + 'px';

  ctx = canvas.getContext('2d');
  if (!ctx) return;
  // 缩放绘图上下文以匹配设备像素比
  ctx.scale(dpr, dpr);

  // 粒子数量：根据容器面积动态调整（但固定为 40 个，足够视觉丰富且性能良好）
  const count = Math.min(40, Math.floor((rect.width * rect.height) / 15000) + 20);
  particles = [];
  for (let i = 0; i < count; i++) {
    particles.push({
      x: Math.random() * rect.width,
      y: Math.random() * rect.height,
      radius: 2 + Math.random() * 2, // 2~5px
      speedY: 0.5 + Math.random() * 1.5,
      speedX: -0.3 + Math.random() * 0.6, // 轻微水平飘动
    });
  }

  // 开始动画
  if (animationFrameId) cancelAnimationFrame(animationFrameId);
  animate();
};

// 动画循环
const animate = () => {
  if (!ctx || !canvasRef.value) return;
  const canvas = canvasRef.value;
  const rect = canvas.getBoundingClientRect();
  const width = rect.width;
  const height = rect.height;

  // 清空画布（透明背景，让下层内容显示）
  ctx.clearRect(0, 0, width, height);

  // 绘制所有雪花（无透明度，纯色）
  ctx.fillStyle = '#AFDAEF';
  for (const p of particles) {
    ctx.beginPath();
    ctx.arc(p.x, p.y, p.radius, 0, Math.PI * 2);
    ctx.fill();
  }

  // 更新位置
  for (const p of particles) {
    p.y += p.speedY;
    p.x += p.speedX;

    // 边界重置（到底部或左右溢出）
    if (p.y > height + 20) {
      p.y = -10;
      p.x = Math.random() * width;
      p.radius = 2 + Math.random() * 6;
      p.speedY = 0.5 + Math.random() * 1.5;
      p.speedX = -0.3 + Math.random() * 0.6;
    }
    if (p.x < -20) p.x = width + 10;
    if (p.x > width + 20) p.x = -10;
  }

  animationFrameId = requestAnimationFrame(animate);
};

// 窗口尺寸变化时重置 Canvas
const handleResize = () => {
  initCanvas();
};

onMounted(async () => {
  await fetchVersion();
  // 等待 DOM 渲染完成，确保容器尺寸已定
  await nextTick();
  initCanvas();
  window.addEventListener('resize', handleResize);
});

onUnmounted(() => {
  if (animationFrameId) {
    cancelAnimationFrame(animationFrameId);
    animationFrameId = null;
  }
  window.removeEventListener('resize', handleResize);
  particles = [];
  ctx = null;
});
</script>

<i18n>
en:
  app: Phi TKC
  license: Licensed by GPLv3
  footer:
    copyright: © {year} Phi TKC. All rights reserved.
zh-CN:
  app: Phi TKC
  license: 基于 GPLv3 协议授权
  footer:
    copyright: © {year} Phi TKC. 保留所有权利。
</i18n>

<style scoped>
/* 容器：居中布局，暗色背景 */
.about-container {
  width: 100%;
  height: 100vh;
  max-width: 1200px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  position: relative;
  overflow: hidden;
  padding: 24px;
  background-color: #ffffff00;
}

/* Canvas 雪花层：覆盖容器，不干扰交互 */
.snow-canvas {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 0; /* 放在内容后面，但若想雪花浮在内容上可改为 z-index: 2，但不影响点击 */
  display: block;
}

/* 确保内容在雪花之上，且可点击 */
.about-content {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  align-items: left;
  gap: 40px;
  width: 100%;
  max-width: 450px;
}
/* 应用头部：Logo 和版本标签 */
.app-header {
  display: flex;
  flex-direction: column;
  align-items: left;
  gap: 16px;
  opacity: 0;
  transform: translateY(-20px);
  animation: headerAppear 0.7s cubic-bezier(0.25, 0.46, 0.45, 0.94) forwards;
  animation-delay: 0.2s;
}
@keyframes headerAppear {
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
.app-logo-img {
  width: 120px;
  max-width: 70vw;
  height: auto;
  object-fit: contain;
  border-radius: 16px;
}
.app-logo-img:hover {
  border-radius: 16px;
}
.version-badge {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 14px;
  width: 66px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  font-size: 0.85rem;
  color: rgba(255, 255, 255, 0.85);
}
/* 信息卡片容器：纵向排列 */
.info-cards {
  display: flex;
  flex-direction: column;
  gap: 14px;
  width: 100%;
}
/* 卡片基础样式与进入动画 */
.info-card {
  backdrop-filter: blur(3px);
  border: 1px solid rgba(255, 255, 255, 0.08);
  overflow: hidden;
  cursor: pointer;
  opacity: 0;
  transform: translateX(-30px);
  animation: cardSlideIn 0.55s cubic-bezier(0.25, 0.46, 0.45, 0.94) forwards;
  animation-delay: .2s;
  transition:
    transform 0.25s ease-in-out,
    box-shadow 0.25s ease-in-out,
    border-color 0.25s ease-in-out;
}
/* 偶数卡片从右侧滑入，增加节奏变化 */
.info-card:nth-child(none) {
  transform: translateX(30px);
}
@keyframes cardSlideIn {
  to {
    opacity: 1;
    transform: translateX(0);
  }
}
/* 卡片内部布局 */
.card-content {
  display: flex;
  align-items: center;
  padding: 18px 20px;
  gap: 16px;
}
.card-icon {
  width: 50px;
  height: 50px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.05);
  flex-shrink: 0;
}
.card-text {
  flex: 1;
  min-width: 0;
}
.card-title {
  font-size: 1.05rem;
  font-weight: 600;
  color: white;
  margin: 0 0 4px 0;
}
.card-subtitle {
  font-size: 0.85rem;
  color: rgba(255, 255, 255, 0.65);
  overflow: hidden;
}
.card-link {
  color: rgba(255, 255, 255, 0.4);
  transition:
    transform 0.2s ease-in-out,
    color 0.2s ease-in-out;
}
.info-card:hover .card-link {
  transform: translateX(4px);
  color: rgba(255, 255, 255, 0.85);
}
/* 底部版权 */
.about-footer {
  margin-top: 8px;
  animation: fadeIn 0.5s ease forwards;
  animation-delay: 1.4s;
  align-items: center;
  padding: 4px 0px;
  width: 200px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  font-size: 0.85rem;
  color: rgba(255, 255, 255, 0.85);
}
@keyframes fadeIn {
  to {
    opacity: 1;
  }
}
.footer-sec {
  font-size: 0.8rem;
  color: rgba(255, 255, 255, 0.45);
  margin:0;
  text-align: center;
}
/* 响应式微调 */
@media (max-width: 768px) {
  .about-container {
    padding: 16px;
  }
  .app-logo-img {
    width: 100px;
  }
  .card-content {
    padding: 14px 16px;
  }
}
@media (max-width: 480px) {
  .about-content {
    gap: 28px;
  }
  .info-cards {
    gap: 10px;
  }
}
</style>
