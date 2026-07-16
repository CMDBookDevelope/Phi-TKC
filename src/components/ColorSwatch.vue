<template>
  <div
    class="mspaint-color-swatch"
    :class="{ active }"
    :style="wrapStyle"
    @click="$emit('click')"
  >
    <!-- 内层颜色圆 + 顶部高光反光 -->
    <div class="color-inner" :style="innerStyle">
      <div class="highlight"></div>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue'

const props = defineProps({
  // 填充色 #xxx / rgb / rgba
  color: {
    type: String,
    required: true
  },
  // 整体外圆直径
  size: {
    type: Number,
    default: 32
  },
  // 是否激活（白色高亮外圈，代表当前选中）
  active: {
    type: Boolean,
    default: false
  }
})

const wrapStyle = computed(() => {
  const s = props.size
  return {
    width: `${s}px`,
    height: `${s}px`,
  }
})

const innerStyle = computed(() => {
  const innerSize = props.size - 6
  return {
    width: `${innerSize}px`,
    height: `${innerSize}px`,
    backgroundColor: props.color,
  }
})
</script>

<style scoped>
.mspaint-color-swatch {
  position: relative;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  /* 外层灰色浮雕环，复刻Windows原生凹凸边框 */
  box-shadow:
    inset 0 0 0 2px #fff,
    inset 0 0 0 3px #999,
    0 1px 2px rgba(0,0,0,0.15);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: transform 0.12s ease;
}
/* 悬浮放大效果 */
.mspaint-color-swatch:hover {
  transform: scale(1.08);
}
.color-inner {
  border-radius: 50%;
  position: relative;
  overflow: hidden;
}
/* 顶部玻璃高光渐变，还原Windows原生反光 */
</style>