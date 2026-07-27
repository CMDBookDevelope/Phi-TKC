<i18n>
en:
  not-binded: You have not binded RPE yet
  bind: Bind RPE
  binded: Binded successfully
  unbind: Unbind RPE
  unbinded: Unbinded successfully
  rpe-folder: Please select RPE's folder
  render: Render

zh-CN:
  not-binded: 你还没有绑定 RPE
  bind: 绑定 RPE
  binded: 绑定成功
  unbind: 解绑 RPE
  unbinded: 解绑成功
  rpe-folder: 请选择 RPE 所在文件夹
  render: 渲染
</i18n>

<script setup lang="ts">
import { mainColor, bgColor, subColor } from '@/colorStore'
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

import type { RPEChart } from './model'
import router from './router'

const { t } = useI18n()

const charts = ref<RPEChart[] | null>(null)

const getRPECharts = async () => {
  try {
    charts.value = (await invoke('get_rpe_charts')) as RPEChart[] | null
  } catch (e) {
    showToast(e, 'error')
    charts.value = null
  }
}

const bindRPE = async () => {
  const file = await open({ directory: true, title: t('rpe-folder') })
  if (!file) return
  try {
    await invoke('set_rpe_dir', { path: file })
    toast(t('binded'), 'success')
    await getRPECharts()
  } catch (e) {
    showToast(e, 'error')
  }
}

const unbindRPE = async () => {
  try {
    await invoke('unset_rpe_dir')
    toast(t('unbinded'), 'success')
    charts.value = null
  } catch (e) {
    showToast(e, 'error')
  }
}

onMounted(() => {
  getRPECharts()
})
</script>

<template>
  <div class="rpe-container">
    <!-- Unbinded state -->
    <template v-if="!charts">
      <div class="glass fluent-glass empty-state">
        <h2 class="empty-title">{{ t('not-binded') }}</h2>
        <fv-button :background="mainColor" :foreground="bgColor" :border-radius="20" @click="bindRPE">
          <fv-AnimatedIcon :font-size=16 icon="Link" modelValue="bounceRotate" :customize-animation="true" />
          <span>{{ t('bind') }}</span>
        </fv-button>
      </div>
    </template>

    <!-- Binded state -->
    <template v-else>
      <div class="rpe-header">
        <fv-button :background="bgColor" :foreground="subColor" :border-radius="20" @click="unbindRPE">
          <fv-AnimatedIcon :font-size=16 icon="Cancel" modelValue="bounceRotate" :customize-animation="true" />
          <span>{{ t('unbind') }}</span>
        </fv-button>
      </div>

      <div class="chart-list">
        <div
          v-for="chart in charts"
          :key="chart.id"
          class="chart-card"
        >
          <div class="chart-cover">
            <div
              class="cover-image"
              :style="{ backgroundImage: 'url(' + convertFileSrc(chart.illustration) + ')' }"
            ></div>
          </div>
          <div class="chart-content">
            <h3 class="chart-name">{{ chart.name }}</h3>
            <p class="chart-id">{{ chart.id }}</p>
            <div class="chart-action">
              <fv-button :background="mainColor" :foreground="bgColor" :border-radius="20" @click="router.push({ name: 'render', query: { chart: chart.path } })">
                <fv-AnimatedIcon :font-size=16 icon="Play" modelValue="bounceRotate" :customize-animation="true" />
                <span>{{ t('render') }}</span>
              </fv-button>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.rpe-container {
  padding: 24px;
  width: 100%;
  height: 100vh;
  max-width: 1100px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

/* ===== Empty State ===== */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 24px;
}
.empty-title {
  font-size: 22px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb), 0.7);
  margin: 0;
  text-align: center;
}

/* ===== Header ===== */
.rpe-header {
  display: flex;
  justify-content: flex-end;
  flex-shrink: 0;
}

/* ===== Chart List ===== */
.chart-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
  flex: 1;
}

/* ===== Chart Card ===== */
.chart-card {
  display: flex;
  flex-direction: row;
  background: rgba(25, 25, 25, 0.85);
  backdrop-filter: blur(8px);
  border: 1px solid rgba(var(--main-color-rgb), 0.08);
  border-radius: 20px;
  overflow: hidden;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}
.chart-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  border-color: rgba(var(--main-color-rgb), 0.12);
}
.chart-cover {
  width: 35%;
  min-height: 180px;
  background: rgba(0, 0, 0, 0.15);
}
.cover-image {
  width: 100%;
  height: 100%;
  background-position: center;
  background-repeat: no-repeat;
  background-size: cover;
}
.chart-content {
  width: 65%;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.chart-name {
  font-size: 18px;
  font-weight: 600;
  color: rgba(var(--main-color-rgb), 0.9);
  margin: 0;
}
.chart-id {
  font-size: 12px;
  color: rgba(var(--main-color-rgb), 0.4);
  margin: 0;
  font-family: 'Consolas', monospace;
}
.chart-action {
  margin-top: auto;
  display: flex;
  justify-content: flex-end;
}

/* ===== Responsive ===== */
@media (max-width: 600px) {
  .rpe-container { padding: 16px; }
  .chart-card { flex-direction: column; }
  .chart-cover { width: 100%; min-height: 160px; }
  .chart-content { width: 100%; }
}
</style>