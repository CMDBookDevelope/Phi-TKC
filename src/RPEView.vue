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
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

import { toast, toastError } from './common'
import type { RPEChart } from './model'
import router from './router'

const { t } = useI18n()

const charts = ref<RPEChart[] | null>(null)

const getRPECharts = async () => {
  try {
    charts.value = (await invoke('get_rpe_charts')) as RPEChart[] | null
  } catch (e) {
    toastError(e)
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
    toastError(e)
  }
}

const unbindRPE = async () => {
  try {
    await invoke('unset_rpe_dir')
    toast(t('unbinded'), 'success')
    charts.value = null
  } catch (e) {
    toastError(e)
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
      <div class="empty-state glass fluent-glass" v-glass>
        <h2 class="empty-title">{{ t('not-binded') }}</h2>
        <v-btn color="primary" @click="bindRPE">
          <v-icon icon="mdi-link-variant" size="18" class="mr-1" />
          <span>{{ t('bind') }}</span>
        </v-btn>
      </div>
    </template>

    <!-- Binded state -->
    <template v-else>
      <div class="rpe-header">
        <v-btn variant="text" class="unbind-btn" @click="unbindRPE">
          <v-icon icon="mdi-link-off" size="18" class="mr-1" />
          <span>{{ t('unbind') }}</span>
        </v-btn>
      </div>

      <div class="chart-list">
        <div v-for="chart in charts" :key="chart.id" class="chart-card glass fluent-glass" v-glass>
          <div class="chart-cover">
            <div class="cover-image" :style="{ backgroundImage: 'url(' + convertFileSrc(chart.illustration) + ')' }"></div>
          </div>
          <div class="chart-content">
            <h3 class="chart-name">{{ chart.name }}</h3>
            <p class="chart-id">{{ chart.id }}</p>
            <div class="chart-action">
              <v-btn color="primary" @click="router.push({ name: 'render', query: { chart: chart.path } })">
                <v-icon icon="mdi-play-circle-outline" size="18" class="mr-1" />
                <span>{{ t('render') }}</span>
              </v-btn>
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
  color: rgba(var(--main-color-rgb), 0.75);
  margin: 0;
  text-align: center;
}

/* ===== Header ===== */
.rpe-header {
  display: flex;
  justify-content: flex-end;
  flex-shrink: 0;
}

.unbind-btn {
  color: rgba(var(--sub-color-rgb), 0.9) !important;
}

/* ===== Chart List ===== */
.chart-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
  flex: 1;
}

.chart-card {
  display: flex;
  flex-direction: row;
  overflow: hidden;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}

.chart-card:hover {
  transform: translateY(-2px);
}

.chart-cover {
  width: 35%;
  min-height: 180px;
  background: rgba(var(--bg-color-rgb), 0.2);
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
  color: var(--main-color);
  margin: 0;
}

.chart-id {
  font-size: 12px;
  color: rgba(var(--main-color-rgb), 0.45);
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
  .rpe-container {
    padding: 16px;
  }
  .chart-card {
    flex-direction: column;
  }
  .chart-cover {
    width: 100%;
    min-height: 160px;
  }
  .chart-content {
    width: 100%;
  }
}
</style>
