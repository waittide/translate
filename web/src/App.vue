<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { storeToRefs } from 'pinia'
import { Toaster } from 'vue-sonner'
import {
  LuActivity,
  LuAlertTriangle,
  LuBot,
  LuCheckCircle,
  LuFolderOpen,
  LuInfo,
  LuLanguages,
  LuLoader2,
  LuX,
  LuXCircle,
} from 'vue-icons-plus/lu'

import AppearancePicker from '@/components/AppearancePicker.vue'
import { useAppToast } from '@/composables/useAppToast'
import { useWorkspaceStore } from '@/stores/workspace'

const route = useRoute()
const workspace = useWorkspaceStore()
const { notify } = useAppToast()
const { connectionState } = storeToRefs(workspace)

const nav = [
  { name: 'projects', label: '项目管理', icon: LuFolderOpen },
  { name: 'workbench', label: '翻译工作台', icon: LuActivity },
  { name: 'providers', label: '模型接口', icon: LuBot },
]

const connection = computed(() => {
  switch (connectionState.value) {
    case 'connected':
      return { className: 'connected', label: '已连接' }
    case 'connecting':
      return { className: 'connecting', label: '正在连接' }
    case 'reconnecting':
      return { className: 'reconnecting', label: '正在重连' }
    default:
      return { className: 'disconnected', label: '未连接' }
  }
})

onMounted(async () => {
  try {
    await Promise.all([workspace.loadProjects(), workspace.loadProviders()])
  } catch (error) {
    notify('error', '无法连接后端', error instanceof Error ? error.message : String(error))
  }
})
</script>

<template>
  <Toaster
    position="top-right"
    close-button-position="top-right"
    container-aria-label="应用通知"
    :visible-toasts="3"
    :toast-options="{ closeButtonAriaLabel: '关闭通知' }"
    rich-colors
    close-button
  >
    <template #success-icon><LuCheckCircle :size="16" aria-hidden="true" /></template>
    <template #info-icon><LuInfo :size="16" aria-hidden="true" /></template>
    <template #warning-icon><LuAlertTriangle :size="16" aria-hidden="true" /></template>
    <template #error-icon><LuXCircle :size="16" aria-hidden="true" /></template>
    <template #loading-icon
      ><span class="spin"><LuLoader2 :size="16" aria-hidden="true" /></span
    ></template>
    <template #close-icon><LuX :size="14" aria-hidden="true" /></template>
  </Toaster>

  <div class="translate-shell">
    <aside class="translate-sidebar">
      <div class="brand">
        <span class="brand-mark"><LuLanguages :size="21" aria-hidden="true" /></span>
        <span class="brand-copy"><strong>Translate</strong><span>本地化翻译工作台</span></span>
      </div>

      <nav class="sidebar-nav" aria-label="主导航">
        <RouterLink
          v-for="item in nav"
          :key="item.name"
          class="nav-item"
          :class="{ active: route.name === item.name }"
          :to="{ name: item.name }"
          :aria-current="route.name === item.name ? 'page' : undefined"
        >
          <component :is="item.icon" :size="16" aria-hidden="true" />
          <span>{{ item.label }}</span>
        </RouterLink>
      </nav>

      <div class="sidebar-footer">
        <div class="connection" :class="connection.className">
          <span aria-hidden="true"></span>
          {{ connection.label }}
        </div>
        <AppearancePicker />
      </div>
    </aside>

    <main class="translate-main">
      <div class="page-content">
        <RouterView />
      </div>
    </main>
  </div>
</template>
