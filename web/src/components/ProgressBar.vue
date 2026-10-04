<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    /** 0-100 的完成百分比 */
    value?: number
    /** 填充色语义：accent 强调 / warning 警告（如停止中） */
    tone?: 'accent' | 'warning'
  }>(),
  {
    value: 0,
    tone: 'accent',
  },
)

const fillStyle = computed(() => ({
  width: `${Math.min(100, Math.max(0, props.value))}%`,
}))
</script>

<template>
  <div
    class="progress-track"
    role="progressbar"
    :aria-valuenow="value"
    aria-valuemin="0"
    aria-valuemax="100"
  >
    <div class="progress-fill" :class="tone" :style="fillStyle"></div>
  </div>
</template>

<style scoped>
.progress-track {
  height: 8px;
  overflow: hidden;
  background: var(--card-raised);
  border: 1px solid var(--border);
  border-radius: 999px;
}
.progress-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 999px;
  transition: width 200ms ease;
}
.progress-fill.warning {
  background: var(--warning);
}
</style>
