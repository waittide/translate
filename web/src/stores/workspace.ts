import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

import {
  api,
  eventUrl,
  type CreateProjectBody,
  type ExportMode,
  type ItemEvent,
  type ItemStatus,
  type ItemView,
  type LlmConfig,
  type ProjectConfig,
  type ProjectView,
  type RunAction,
  type StatusEvent,
} from '@/api/client'

export type ConnectionState = 'connecting' | 'connected' | 'reconnecting' | 'disconnected'
export type ItemFilter = ItemStatus | 'all'

function parseJson<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T
  } catch {
    return null
  }
}

export const useWorkspaceStore = defineStore('workspace', () => {
  const project = ref<ProjectView | null>(null)
  const recent = ref<string[]>([])
  const providers = ref<Record<string, LlmConfig>>({})
  const items = ref<ItemView[]>([])
  const itemsTotal = ref(0)
  const itemsLoading = ref(false)
  const loading = ref(false)
  const connectionState = ref<ConnectionState>('disconnected')
  const statusFilter = ref<ItemFilter>('all')
  const page = ref(0)
  const pageSize = 25

  let source: EventSource | null = null

  const counts = computed(() => project.value?.counts ?? null)
  const pageCount = computed(() => Math.max(1, Math.ceil(itemsTotal.value / pageSize)))

  function connectEvents() {
    source?.close()
    if (!project.value) {
      source = null
      connectionState.value = 'disconnected'
      return
    }
    connectionState.value = 'connecting'
    const next = new EventSource(eventUrl())
    source = next
    next.onopen = () => {
      if (source === next) connectionState.value = 'connected'
    }
    next.onerror = () => {
      if (source !== next) return
      connectionState.value =
        next.readyState === EventSource.CLOSED ? 'disconnected' : 'reconnecting'
    }
    next.addEventListener('progress', (event) => {
      if (source !== next) return
      const data = parseJson<ProjectView>((event as MessageEvent).data)
      if (data) project.value = data
    })
    next.addEventListener('status', (event) => {
      if (source !== next) return
      const data = parseJson<StatusEvent>((event as MessageEvent).data)
      if (data && project.value) {
        project.value = {
          ...project.value,
          info: { ...project.value.info, run_status: data.status },
        }
      }
    })
    next.addEventListener('item', (event) => {
      if (source !== next) return
      const data = parseJson<ItemEvent>((event as MessageEvent).data)
      if (data) applyItemEvent(data)
    })
  }

  function applyItemEvent(event: ItemEvent) {
    const index = items.value.findIndex(
      (item) => item.file === event.file && item.line === event.line,
    )
    if (index < 0) {
      // 条目刚进入当前筛选状态且不在本页列表：事件携带原文，直接插入渲染（事件驱动实时）
      if (matchesFilter(event.status) && statusFilter.value !== 'all') {
        items.value.push({
          file: event.file,
          line: event.line,
          status: event.status,
          source: event.source,
          target: event.status === 'success' ? (event.target ?? null) : null,
          reason: event.status === 'failure' ? (event.reason ?? null) : null,
        })
        items.value.sort((a, b) => (a.file === b.file ? a.line - b.line : a.file < b.file ? -1 : 1))
        if (items.value.length > pageSize) items.value.length = pageSize
        itemsTotal.value += 1
      }
      return
    }
    const current = items.value[index]!
    if (matchesFilter(event.status)) {
      items.value[index] = {
        ...current,
        status: event.status,
        // 翻译中/等待中无译文与原因
        target: event.status === 'translating' ? null : (event.target ?? current.target),
        reason: event.status === 'translating' ? null : (event.reason ?? current.reason),
      }
    } else {
      items.value.splice(index, 1)
      itemsTotal.value = Math.max(0, itemsTotal.value - 1)
    }
  }

  function matchesFilter(status: ItemStatus) {
    return statusFilter.value === 'all' || statusFilter.value === status
  }

  async function loadProjects() {
    loading.value = true
    try {
      const data = await api.projectList()
      project.value = data.current
      recent.value = data.recent
      connectEvents()
      if (data.current) await loadItems()
    } finally {
      loading.value = false
    }
  }

  async function createProject(
    body: CreateProjectBody,
    onProgress?: (scanned: number, total: number) => void,
  ): Promise<ProjectView> {
    const created = await api.createProject(body, onProgress ?? (() => {}))
    project.value = created
    await syncRecent()
    statusFilter.value = 'all'
    page.value = 0
    await loadItems()
    return created
  }

  async function loadProject(transPath: string): Promise<ProjectView> {
    const loaded = await api.loadProject(transPath)
    project.value = loaded
    await syncRecent()
    statusFilter.value = 'all'
    page.value = 0
    await loadItems()
    return loaded
  }

  async function syncRecent() {
    const data = await api.projectList()
    recent.value = data.recent
    connectEvents()
  }

  async function run(action: RunAction) {
    project.value = await api.run(action)
    if (action === 'start') await loadItems()
  }

  async function updateProjectConfig(config: ProjectConfig) {
    project.value = await api.updateProjectConfig(config)
  }

  async function loadItems() {
    itemsLoading.value = true
    try {
      const data = await api.items({
        status: statusFilter.value,
        offset: page.value * pageSize,
        limit: pageSize,
      })
      items.value = data.items
      itemsTotal.value = data.total
    } finally {
      itemsLoading.value = false
    }
  }

  function setStatusFilter(filter: ItemFilter) {
    if (filter === statusFilter.value) return
    statusFilter.value = filter
    page.value = 0
    void loadItems()
  }

  function setPage(next: number) {
    if (next === page.value) return
    page.value = next
    void loadItems()
  }

  async function patchItem(file: string, line: number, target: string) {
    project.value = await api.patchItem(file, line, target)
    const index = items.value.findIndex((item) => item.file === file && item.line === line)
    if (index >= 0) items.value[index] = { ...items.value[index]!, target }
  }

  async function resetItem(file: string, line: number) {
    project.value = await api.resetItem(file, line)
    moveItemToWaiting(file, line)
  }

  async function retryItem(file: string, line: number) {
    await api.retryItem(file, line)
    moveItemToWaiting(file, line)
  }

  function moveItemToWaiting(file: string, line: number) {
    const index = items.value.findIndex((item) => item.file === file && item.line === line)
    if (index < 0) return
    if (statusFilter.value === 'all' || statusFilter.value === 'waiting') {
      items.value[index] = {
        ...items.value[index]!,
        status: 'waiting',
        target: null,
        reason: null,
      }
    } else {
      items.value.splice(index, 1)
      itemsTotal.value = Math.max(0, itemsTotal.value - 1)
    }
  }

  async function retryAll() {
    const reply = await api.retryAll()
    await loadItems()
    return reply
  }

  async function exportProject(mode: ExportMode, outputDir: string) {
    return api.exportProject(mode, outputDir)
  }

  async function loadProviders() {
    providers.value = await api.providers()
  }

  async function saveProvider(name: string, config: LlmConfig, isNew: boolean) {
    if (isNew) await api.upsertProvider(name, config)
    else await api.updateProvider(name, config)
    await loadProviders()
  }

  async function deleteProvider(name: string) {
    await api.deleteProvider(name)
    await loadProviders()
  }

  async function fetchModels(name: string) {
    return api.providerModels(name)
  }

  async function fetchModelsByConfig(config: LlmConfig) {
    return api.providerModelsByConfig(config)
  }

  return {
    project,
    recent,
    providers,
    items,
    itemsTotal,
    itemsLoading,
    loading,
    connectionState,
    statusFilter,
    page,
    pageSize,
    counts,
    pageCount,
    loadProjects,
    createProject,
    loadProject,
    run,
    updateProjectConfig,
    loadItems,
    setStatusFilter,
    setPage,
    patchItem,
    resetItem,
    retryItem,
    retryAll,
    exportProject,
    loadProviders,
    saveProvider,
    deleteProvider,
    fetchModels,
    fetchModelsByConfig,
    connectEvents,
  }
})
