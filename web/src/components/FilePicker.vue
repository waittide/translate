<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { LuArrowUp, LuFileText, LuFolder, LuFolderOpen, LuLoader2, LuX } from 'vue-icons-plus/lu'

import { api, type FsEntry } from '@/api/client'

const props = defineProps<{
  mode: 'file' | 'folder'
}>()

const emit = defineEmits<{
  select: [path: string]
  close: []
}>()

const dialog = ref<HTMLElement | null>(null)
const path = ref('')
const parent = ref<string | null>(null)
const entries = ref<FsEntry[]>([])
const selectedName = ref('')
const loading = ref(false)
const error = ref('')

const title = props.mode === 'file' ? '选择文件' : '选择文件夹'

async function load(next: string) {
  loading.value = true
  error.value = ''
  selectedName.value = ''
  try {
    const data = await api.fsList(next)
    path.value = data.path
    parent.value = data.parent
    entries.value = data.entries
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    entries.value = []
  } finally {
    loading.value = false
  }
}

function goUp() {
  if (parent.value) void load(parent.value)
}

function enterDir(name: string) {
  void load(`${path.value}/${name}`)
}

function pick() {
  if (props.mode === 'file') {
    const target = entries.value.find((entry) => entry.name === selectedName.value)
    if (!target || target.type !== 'file') return
    emit('select', `${path.value}/${target.name}`)
  } else {
    const target = entries.value.find((entry) => entry.name === selectedName.value)
    if (target && target.type === 'dir') {
      emit('select', `${path.value}/${target.name}`)
    } else {
      emit('select', path.value)
    }
  }
}

function onRowClick(entry: FsEntry) {
  if (entry.type === 'dir' && props.mode === 'file') {
    enterDir(entry.name)
  } else {
    selectedName.value = entry.name
  }
}

function onRowDblClick(entry: FsEntry) {
  if (entry.type === 'dir') {
    enterDir(entry.name)
  } else if (props.mode === 'file') {
    emit('select', `${path.value}/${entry.name}`)
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') emit('close')
}

function close() {
  emit('close')
}

onMounted(() => {
  dialog.value?.focus()
  document.body.style.overflow = 'hidden'
  void load('')
})

onBeforeUnmount(() => {
  document.body.style.overflow = ''
})
</script>

<template>
  <Teleport to="body">
    <div class="file-picker-overlay" @click.self="close">
      <section
        ref="dialog"
        class="file-picker-panel"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        tabindex="-1"
        @keydown="onKeydown"
      >
        <header class="file-picker-header">
          <span class="file-picker-icon" aria-hidden="true">
            <LuFolderOpen :size="16" />
          </span>
          <div class="file-picker-title">
            <strong>{{ title }}</strong>
            <small>{{
              mode === 'file' ? '选择要翻译的 txt 文件' : '选择包含 txt 文件的文件夹'
            }}</small>
          </div>
          <button
            class="button ghost icon-button"
            type="button"
            aria-label="关闭"
            title="关闭"
            @click="close"
          >
            <LuX :size="15" aria-hidden="true" />
          </button>
        </header>

        <div class="file-picker-toolbar">
          <button class="button ghost small" type="button" :disabled="!parent" @click="goUp">
            <LuArrowUp :size="13" aria-hidden="true" />
            上级目录
          </button>
          <span class="file-picker-path mono">{{ path || '加载中…' }}</span>
        </div>

        <div class="file-picker-list" role="listbox" :aria-label="`${title}列表`">
          <div v-if="loading" class="file-picker-state">
            <span class="spin"><LuLoader2 :size="16" aria-hidden="true" /></span>
            正在读取目录…
          </div>
          <div v-else-if="error" class="file-picker-state error">{{ error }}</div>
          <div v-else-if="!entries.length" class="file-picker-state">空目录。</div>
          <template v-else>
            <button
              v-for="entry in entries"
              :key="entry.name"
              class="file-picker-row"
              :class="{ selected: selectedName === entry.name }"
              type="button"
              role="option"
              :aria-selected="selectedName === entry.name"
              @click="onRowClick(entry)"
              @dblclick="onRowDblClick(entry)"
            >
              <span class="row-icon" aria-hidden="true">
                <LuFolder v-if="entry.type === 'dir'" :size="15" />
                <LuFileText v-else :size="15" />
              </span>
              <span class="mono">{{ entry.name }}</span>
              <small v-if="entry.type === 'dir'" class="row-kind">文件夹</small>
              <small v-else class="row-kind">文件</small>
            </button>
          </template>
        </div>

        <footer class="file-picker-footer">
          <button class="button ghost" type="button" @click="close">取消</button>
          <button
            class="button primary"
            type="button"
            :disabled="mode === 'file' && !selectedName"
            @click="pick"
          >
            选择
          </button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.file-picker-overlay {
  position: fixed;
  z-index: 300;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 16px;
  overflow-y: auto;
  background: transparent;
}
.file-picker-panel {
  display: flex;
  flex-direction: column;
  width: min(560px, 100%);
  max-height: min(70vh, 560px);
  padding: 16px;
  background: var(--panel);
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  box-shadow: 0 24px 64px -24px var(--shadow);
}
.file-picker-header {
  display: flex;
  align-items: center;
  gap: 10px;
}
.file-picker-icon {
  display: grid;
  width: 32px;
  height: 32px;
  color: var(--accent-readable);
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
  border-radius: 9px;
  place-items: center;
}
.file-picker-title {
  min-width: 0;
  flex: 1 1 auto;
}
.file-picker-title strong,
.file-picker-title small {
  display: block;
}
.file-picker-title strong {
  font-size: 14px;
}
.file-picker-title small {
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 11px;
}
.file-picker-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 14px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border);
}
.file-picker-path {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  color: var(--text-muted);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: right;
}
.file-picker-list {
  min-height: 200px;
  max-height: 340px;
  margin-top: 10px;
  overflow-y: auto;
}
.file-picker-state {
  display: grid;
  place-items: center;
  gap: 8px;
  min-height: 180px;
  color: var(--text-subtle);
  font-size: 12px;
}
.file-picker-state.error {
  color: var(--danger);
  text-align: center;
  word-break: break-word;
}
.file-picker-row {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 8px 10px;
  color: var(--text-muted);
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: 8px;
  cursor: pointer;
}
.file-picker-row:hover,
.file-picker-row.selected {
  color: var(--text);
  background: var(--card-raised);
}
.file-picker-row.selected {
  box-shadow: inset 0 0 0 1px var(--accent);
}
.file-picker-row .mono {
  min-width: 0;
  overflow: hidden;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-picker-row .row-kind {
  color: var(--text-subtle);
  font-size: 10px;
}
.file-picker-row .row-icon {
  width: 28px;
  height: 28px;
  border-radius: 7px;
}
.file-picker-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}
</style>
