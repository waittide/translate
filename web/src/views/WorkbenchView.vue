<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import {
  LuClock,
  LuCoins,
  LuDownload,
  LuGauge,
  LuLoader2,
  LuPause,
  LuPencil,
  LuPlay,
  LuRefreshCw,
  LuRotateCcw,
  LuSave,
  LuX,
} from 'vue-icons-plus/lu'

import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import ProgressBar from '@/components/ProgressBar.vue'
import { useAppToast } from '@/composables/useAppToast'
import { useWorkspaceStore, type ItemFilter } from '@/stores/workspace'
import type { ExportMode, ProjectConfig, RunAction } from '@/api/client'

const router = useRouter()
const workspace = useWorkspaceStore()
const { notify } = useAppToast()
const {
  project,
  counts,
  items,
  itemsTotal,
  itemsLoading,
  statusFilter,
  page,
  pageCount,
  providers,
} = storeToRefs(workspace)

const busyAction = ref<RunAction | null>(null)
const pausing = ref(false)
const retryingAll = ref(false)
const exporting = ref(false)
const outputDir = ref('')
const exportMode = ref<ExportMode>('translated_only')
const editingKey = ref<string | null>(null)
const editDraft = ref('')
const savingConfig = ref(false)
const configDraft = reactive({
  provider: '',
  taskLimit: 4,
  retryLimit: 3,
  timeoutSec: 300,
})
let lastSyncedConfig = ''

const providerOptions = computed<SelectOption[]>(() =>
  Object.keys(providers.value).map((name) => ({
    value: name,
    label: name,
    description: providers.value[name]?.model ?? undefined,
  })),
)

watch(
  () => project.value?.project_config,
  (cfg) => {
    if (!cfg) return
    const key = JSON.stringify(cfg)
    if (key === lastSyncedConfig) return
    lastSyncedConfig = key
    configDraft.provider = cfg.provider
    configDraft.taskLimit = cfg.task_limit
    configDraft.retryLimit = cfg.retry_limit
    configDraft.timeoutSec = Math.round(cfg.task_timeout_ms / 1000)
  },
  { immediate: true },
)

const runStatus = computed(() => project.value?.info.run_status ?? null)

watch(runStatus, (status) => {
  if (status !== 'running') pausing.value = false
})

const showStart = computed(
  () =>
    (runStatus.value === 'notstarted' || runStatus.value === 'completed') &&
    (counts.value?.waiting ?? 0) > 0,
)
const showPause = computed(() => runStatus.value === 'running')
const showResume = computed(() => runStatus.value === 'paused')

const runStatusLabels: Record<string, string> = {
  notstarted: '未开始',
  running: '运行中',
  paused: '已暂停',
  completed: '已完成',
}

const completedTasks = computed(() => (counts.value?.success ?? 0) + (counts.value?.failure ?? 0))
const remainingTasks = computed(() =>
  Math.max(0, (counts.value?.total ?? 0) - completedTasks.value),
)
const inFlightTasks = computed(() => counts.value?.translating ?? 0)
const runProgressPct = computed(() => {
  const total = counts.value?.total ?? 0
  return total === 0 ? 0 : Math.round((completedTasks.value / total) * 100)
})
const showRunProgress = computed(
  () => runStatus.value === 'running' || runStatus.value === 'paused' || pausing.value,
)

const filterTabs: Array<{ value: ItemFilter; label: string }> = [
  { value: 'all', label: '全部' },
  { value: 'waiting', label: '等待中' },
  { value: 'translating', label: '翻译中' },
  { value: 'success', label: '成功' },
  { value: 'failure', label: '失败' },
]

function formatNumber(value: number): string {
  return new Intl.NumberFormat('zh-CN').format(value)
}

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000)
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${pad(minutes)}:${pad(seconds)}`
}

async function runAction(action: RunAction) {
  busyAction.value = action
  if (action === 'pause') pausing.value = true
  try {
    await workspace.run(action)
    if (action === 'pause') {
      notify('info', '正在暂停', '等待进行中的请求完成后停止。')
    } else {
      notify(
        'success',
        action === 'start' ? '翻译已开始' : '翻译已继续',
        action === 'start' ? '进度通过实时事件更新。' : '从暂停处继续翻译。',
      )
    }
  } catch (error) {
    pausing.value = false
    notify('error', '操作失败', error instanceof Error ? error.message : String(error))
  } finally {
    busyAction.value = null
  }
}

async function saveConfig() {
  const config: ProjectConfig = {
    task_limit: Math.max(1, Math.floor(configDraft.taskLimit)),
    retry_limit: Math.max(0, Math.floor(configDraft.retryLimit)),
    task_timeout_ms: Math.max(1000, Math.floor(configDraft.timeoutSec) * 1000),
    provider: configDraft.provider,
  }
  if (!config.provider) {
    notify('warning', '请选择模型接口', '运行配置需要引用一个已配置的 provider。')
    return
  }
  savingConfig.value = true
  try {
    await workspace.updateProjectConfig(config)
    notify('success', '配置已保存', '下次启动翻译时生效。')
  } catch (error) {
    notify('error', '保存配置失败', error instanceof Error ? error.message : String(error))
  } finally {
    savingConfig.value = false
  }
}

async function retryAll() {
  retryingAll.value = true
  try {
    const reply = await workspace.retryAll()
    notify('success', '失败条目已重新排队', `已移回 ${reply.retried} 条等待翻译。`)
  } catch (error) {
    notify('error', '重试失败', error instanceof Error ? error.message : String(error))
  } finally {
    retryingAll.value = false
  }
}

function beginEdit(item: { file: string; line: number; target: string | null }) {
  editingKey.value = `${item.file}:${item.line}`
  editDraft.value = item.target ?? ''
}

function cancelEdit() {
  editingKey.value = null
  editDraft.value = ''
}

async function saveEdit(item: { file: string; line: number; target: string | null }) {
  const next = editDraft.value
  if (next !== (item.target ?? '')) {
    try {
      await workspace.patchItem(item.file, item.line, next)
      notify('success', '译文已保存', `${item.file}:${item.line}`)
    } catch (error) {
      notify('error', '保存译文失败', error instanceof Error ? error.message : String(error))
      await workspace.loadItems()
    }
  }
  cancelEdit()
}

async function handleEditKeydown(
  event: KeyboardEvent,
  item: { file: string; line: number; target: string | null },
) {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault()
    await saveEdit(item)
  } else if (event.key === 'Escape') {
    cancelEdit()
  }
}

async function resetItem(item: { file: string; line: number }) {
  try {
    await workspace.resetItem(item.file, item.line)
    notify('info', '条目已重置', `${item.file}:${item.line} 将重新翻译。`)
  } catch (error) {
    notify('error', '重置失败', error instanceof Error ? error.message : String(error))
  }
}

async function retryItem(item: { file: string; line: number }) {
  try {
    await workspace.retryItem(item.file, item.line)
    notify('info', '条目已重新排队', `${item.file}:${item.line}`)
  } catch (error) {
    notify('error', '重试失败', error instanceof Error ? error.message : String(error))
  }
}

async function submitExport() {
  const dir = outputDir.value.trim()
  if (!dir) {
    notify('warning', '请填写输出目录', '导出需要指定目标文件夹路径。')
    return
  }
  exporting.value = true
  try {
    const reply = await workspace.exportProject(exportMode.value, dir)
    notify('success', '导出完成', `已写出 ${reply.files} 个文件到 ${dir}。`)
  } catch (error) {
    notify('error', '导出失败', error instanceof Error ? error.message : String(error))
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <template v-if="project">
    <header class="page-heading">
      <div>
        <p class="eyebrow">Workbench</p>
        <h1>{{ project.info.translate_name ?? project.info.original_name }}</h1>
      </div>
    </header>

    <section class="panel run-panel">
      <div class="run-panel-top">
        <div class="run-project-meta">
          <div class="run-title">
            <strong v-if="project.info.translate_name">{{ project.info.translate_name }}</strong>
            <span class="mono">{{ project.info.object.path }}</span>
          </div>
          <div class="run-meta-row">
            <span>{{ project.info.source_language }} → {{ project.info.target_language }}</span>
            <span>{{ project.info.object.type === 'file' ? '单个文件' : '文件夹' }}</span>
          </div>
        </div>

        <div class="run-controls">
          <button
            v-if="showStart"
            class="button primary"
            type="button"
            :disabled="busyAction !== null"
            @click="runAction('start')"
          >
            <span v-if="busyAction === 'start'" class="spin"
              ><LuLoader2 :size="15" aria-hidden="true"
            /></span>
            <LuPlay v-else :size="15" aria-hidden="true" />
            开始翻译
          </button>
          <button
            v-if="showPause"
            class="button secondary"
            type="button"
            :disabled="busyAction !== null || pausing"
            @click="runAction('pause')"
          >
            <span v-if="busyAction === 'pause' || pausing" class="spin"
              ><LuLoader2 :size="15" aria-hidden="true"
            /></span>
            <LuPause v-else :size="15" aria-hidden="true" />
            暂停
          </button>
          <button
            v-if="showResume"
            class="button secondary"
            type="button"
            :disabled="busyAction !== null"
            @click="runAction('resume')"
          >
            <span v-if="busyAction === 'resume'" class="spin"
              ><LuLoader2 :size="15" aria-hidden="true"
            /></span>
            <LuPlay v-else :size="15" aria-hidden="true" />
            继续
          </button>
          <button
            class="button ghost"
            type="button"
            :disabled="(counts?.failure ?? 0) === 0 || retryingAll"
            @click="retryAll"
          >
            <span v-if="retryingAll" class="spin"><LuLoader2 :size="15" aria-hidden="true" /></span>
            <LuRefreshCw v-else :size="15" aria-hidden="true" />
            重试失败{{ counts?.failure ? `（${counts.failure}）` : '' }}
          </button>
          <span class="status-pill" :class="project.info.run_status">
            {{
              pausing
                ? '暂停中…'
                : (runStatusLabels[project.info.run_status] ?? project.info.run_status)
            }}
          </span>
        </div>
      </div>

      <div class="stat-grid">
        <article>
          <span>总条目</span>
          <strong>{{ formatNumber(counts?.total ?? 0) }}</strong>
        </article>
        <article>
          <span>翻译成功</span>
          <strong class="success-text">{{ formatNumber(counts?.success ?? 0) }}</strong>
        </article>
        <article>
          <span>翻译失败</span>
          <strong class="danger-text">{{ formatNumber(counts?.failure ?? 0) }}</strong>
        </article>
        <article>
          <span>翻译中</span>
          <strong class="translating-text">{{ formatNumber(counts?.translating ?? 0) }}</strong>
        </article>
        <article>
          <span>等待翻译</span>
          <strong>{{ formatNumber(counts?.waiting ?? 0) }}</strong>
        </article>
        <article>
          <span>忽略行</span>
          <strong class="muted">{{ formatNumber(counts?.ignore ?? 0) }}</strong>
        </article>
      </div>

      <div class="run-info-row">
        <span class="run-info-item"
          ><LuClock :size="13" aria-hidden="true" />运行时长
          {{ formatDuration(project.info.running_time_ms) }}</span
        >
        <span class="run-info-item"
          ><LuCoins :size="13" aria-hidden="true" />Token 输入
          {{ formatNumber(project.info.token_usage.input) }} · 输出
          {{ formatNumber(project.info.token_usage.output) }}</span
        >
        <span class="run-info-item"
          ><LuGauge :size="13" aria-hidden="true" />速率 {{ project.token_rate.input.toFixed(1) }} /
          {{ project.token_rate.output.toFixed(1) }} tok/s</span
        >
      </div>
      <div v-if="showRunProgress" class="run-progress">
        <ProgressBar :value="runProgressPct" :tone="pausing ? 'warning' : 'accent'" />
        <p class="progress-label">
          <template v-if="pausing"> 正在停止… 还剩 {{ inFlightTasks }} 个任务在途 </template>
          <template v-else-if="runStatus === 'paused'">
            已暂停 · 剩余 {{ formatNumber(remainingTasks) }} 条任务
          </template>
          <template v-else>
            已翻译 {{ formatNumber(completedTasks) }} / {{ formatNumber(counts?.total ?? 0) }} ·
            剩余 {{ formatNumber(remainingTasks) }} 条
          </template>
        </p>
      </div>
    </section>

    <section class="panel" aria-labelledby="config-heading">
      <div class="panel-heading">
        <div>
          <h2 id="config-heading">工程配置</h2>
          <p>修改运行参数，保存后下次启动翻译时生效。</p>
        </div>
      </div>
      <div class="config-form">
        <div class="field span-2">
          <span class="field-label" id="config-provider-label">模型接口</span>
          <AppSelect v-model="configDraft.provider" :options="providerOptions" searchable />
        </div>
        <div class="field">
          <label class="field-label" for="config-task-limit">并发任务数</label>
          <input
            id="config-task-limit"
            v-model.number="configDraft.taskLimit"
            type="number"
            min="1"
            max="64"
          />
        </div>
        <div class="field">
          <label class="field-label" for="config-retry-limit">失败重试次数</label>
          <input
            id="config-retry-limit"
            v-model.number="configDraft.retryLimit"
            type="number"
            min="0"
            max="10"
          />
        </div>
        <div class="field">
          <label class="field-label" for="config-timeout">单次请求超时（秒）</label>
          <input
            id="config-timeout"
            v-model.number="configDraft.timeoutSec"
            type="number"
            min="1"
            max="3600"
          />
        </div>
        <div class="config-save">
          <button
            class="button secondary"
            type="button"
            :disabled="savingConfig"
            @click="saveConfig"
          >
            <span v-if="savingConfig" class="spin"
              ><LuLoader2 :size="14" aria-hidden="true"
            /></span>
            <LuSave v-else :size="14" aria-hidden="true" />
            保存配置
          </button>
        </div>
      </div>
    </section>

    <section class="panel" aria-labelledby="export-heading">
      <div class="panel-heading">
        <div>
          <h2 id="export-heading">导出译文</h2>
          <p>按文件写出译文，不修改工程文件。</p>
        </div>
      </div>
      <div class="export-form">
        <div class="field export-mode">
          <label class="field-label" for="export-mode">导出模式</label>
          <select id="export-mode" v-model="exportMode">
            <option value="translated_only">仅译文（未成功条目保留原文）</option>
            <option value="bilingual">双语（每行 原文\t译文）</option>
          </select>
        </div>
        <div class="field export-dir">
          <label class="field-label" for="export-dir">输出目录</label>
          <input
            id="export-dir"
            v-model="outputDir"
            type="text"
            placeholder="/path/to/output"
            autocomplete="off"
            spellcheck="false"
          />
        </div>
        <button
          class="button secondary export-submit"
          type="button"
          :disabled="exporting"
          @click="submitExport"
        >
          <span v-if="exporting" class="spin"><LuLoader2 :size="15" aria-hidden="true" /></span>
          <LuDownload v-else :size="15" aria-hidden="true" />
          导出
        </button>
      </div>
    </section>

    <section class="panel" aria-labelledby="items-heading">
      <div class="panel-heading items-heading">
        <div>
          <h2 id="items-heading">条目</h2>
          <p>共 {{ formatNumber(itemsTotal) }} 条 · 按文件与行号排序。</p>
        </div>
        <div class="panel-actions">
          <div class="filter-tabs" role="tablist" aria-label="状态筛选">
            <button
              v-for="tab in filterTabs"
              :key="tab.value"
              type="button"
              role="tab"
              :aria-selected="statusFilter === tab.value"
              :class="{ active: statusFilter === tab.value }"
              @click="workspace.setStatusFilter(tab.value)"
            >
              {{ tab.label }}
            </button>
          </div>
          <button
            class="button ghost icon-button"
            type="button"
            aria-label="刷新条目"
            title="刷新条目"
            :disabled="itemsLoading"
            @click="workspace.loadItems()"
          >
            <span v-if="itemsLoading" class="spin"
              ><LuLoader2 :size="15" aria-hidden="true"
            /></span>
            <LuRefreshCw v-else :size="15" aria-hidden="true" />
          </button>
        </div>
      </div>

      <div class="table-shell">
        <table class="items-table">
          <thead>
            <tr>
              <th class="col-loc">位置</th>
              <th>原文</th>
              <th>译文 / 状态</th>
              <th class="col-actions">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in items" :key="`${item.file}:${item.line}`">
              <td class="col-loc">
                <span class="mono">{{ item.file }}:{{ item.line }}</span>
                <span class="status-pill" :class="item.status">
                  {{
                    item.status === 'success'
                      ? '成功'
                      : item.status === 'failure'
                        ? '失败'
                        : item.status === 'translating'
                          ? '翻译中'
                          : '等待'
                  }}
                </span>
              </td>
              <td class="cell-source">
                <span class="item-text">{{ item.source }}</span>
              </td>
              <td class="cell-target">
                <template v-if="editingKey === `${item.file}:${item.line}`">
                  <textarea
                    v-model="editDraft"
                    class="edit-textarea"
                    rows="2"
                    aria-label="编辑译文"
                    @keydown="handleEditKeydown($event, item)"
                  ></textarea>
                </template>
                <template v-else>
                  <span v-if="item.status === 'success'" class="item-text">{{ item.target }}</span>
                  <span
                    v-else-if="item.status === 'failure'"
                    class="item-reason"
                    :title="item.reason ?? ''"
                  >
                    {{ item.reason }}
                  </span>
                  <span v-else-if="item.status === 'translating'" class="item-text translating-text"
                    >翻译中…</span
                  >
                  <span v-else class="item-text muted">等待翻译</span>
                </template>
              </td>
              <td class="col-actions">
                <div class="row-actions">
                  <template v-if="editingKey === `${item.file}:${item.line}`">
                    <button
                      class="button ghost small row-icon-btn"
                      type="button"
                      aria-label="保存译文"
                      title="保存译文"
                      @click="saveEdit(item)"
                    >
                      <LuSave :size="14" aria-hidden="true" />
                    </button>
                    <button
                      class="button ghost small row-icon-btn"
                      type="button"
                      aria-label="取消编辑"
                      title="取消编辑"
                      @click="cancelEdit"
                    >
                      <LuX :size="14" aria-hidden="true" />
                    </button>
                  </template>
                  <template v-else-if="item.status === 'success'">
                    <button
                      class="button ghost small row-icon-btn"
                      type="button"
                      aria-label="编辑"
                      title="编辑"
                      @click="beginEdit(item)"
                    >
                      <LuPencil :size="14" aria-hidden="true" />
                    </button>
                    <button
                      class="button ghost small row-icon-btn"
                      type="button"
                      aria-label="重置"
                      title="重置"
                      @click="resetItem(item)"
                    >
                      <LuRotateCcw :size="14" aria-hidden="true" />
                    </button>
                  </template>
                  <template v-else-if="item.status === 'failure'">
                    <button
                      class="button ghost small row-icon-btn"
                      type="button"
                      aria-label="重试"
                      title="重试"
                      @click="retryItem(item)"
                    >
                      <LuRefreshCw :size="14" aria-hidden="true" />
                    </button>
                    <button
                      class="button ghost small row-icon-btn"
                      type="button"
                      aria-label="重置"
                      title="重置"
                      @click="resetItem(item)"
                    >
                      <LuRotateCcw :size="14" aria-hidden="true" />
                    </button>
                  </template>
                  <span v-else class="muted">—</span>
                </div>
              </td>
            </tr>
            <tr v-if="itemsLoading">
              <td colspan="4" class="table-loading">
                <span class="spin"><LuLoader2 :size="15" aria-hidden="true" /></span>
                加载条目…
              </td>
            </tr>
            <tr v-else-if="!items.length">
              <td colspan="4" class="table-loading muted">
                {{ statusFilter === 'all' ? '暂无条目。' : '没有符合条件的条目。' }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div class="pager">
        <span class="muted">
          第 {{ page + 1 }} / {{ pageCount }} 页 · 共 {{ formatNumber(itemsTotal) }} 条
        </span>
        <div class="pager-buttons">
          <button
            class="button ghost small"
            type="button"
            :disabled="page === 0 || itemsLoading"
            @click="workspace.setPage(page - 1)"
          >
            上一页
          </button>
          <button
            class="button ghost small"
            type="button"
            :disabled="page + 1 >= pageCount || itemsLoading"
            @click="workspace.setPage(page + 1)"
          >
            下一页
          </button>
        </div>
      </div>
    </section>
  </template>

  <section v-else class="empty-state">
    <h2>尚未打开工程</h2>
    <p>创建或载入一个工程后，才能开始翻译。</p>
    <RouterLink class="button primary" :to="{ name: 'projects' }">前往项目管理</RouterLink>
  </section>
</template>

<style scoped>
.run-panel-top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 20px;
  flex-wrap: wrap;
}
.run-project-meta {
  min-width: 0;
}
.run-title {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
  flex-wrap: wrap;
}
.run-title strong {
  font-size: 15px;
}
.run-meta-row {
  display: flex;
  gap: 12px;
  margin-top: 4px;
  color: var(--text-muted);
  font-size: 12px;
}
.run-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.run-info-row {
  display: flex;
  gap: 18px;
  margin-top: 14px;
  padding-top: 14px;
  color: var(--text-subtle);
  font-size: 11px;
  border-top: 1px solid var(--border);
  flex-wrap: wrap;
}
.run-info-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.run-progress {
  display: grid;
  gap: 8px;
  margin-top: 14px;
}
.run-progress .progress-label {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}
.stat-grid {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 12px;
  margin-top: 18px;
}
.stat-grid article {
  padding: 12px 14px;
  background: var(--card-raised);
  border: 1px solid var(--border);
  border-radius: 11px;
}
.stat-grid span,
.stat-grid strong {
  display: block;
}
.stat-grid span {
  color: var(--text-subtle);
  font-size: 11px;
}
.stat-grid strong {
  margin-top: 4px;
  font-size: 20px;
}
.export-form {
  display: grid;
  grid-template-columns: minmax(200px, 0.8fr) minmax(0, 1.2fr) auto;
  gap: 12px;
  align-items: end;
}
.config-form {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  align-items: end;
}
.config-save {
  justify-self: end;
}
.export-submit {
  min-height: 40px;
}
.items-heading {
  flex-wrap: wrap;
}
.filter-tabs {
  display: inline-flex;
  gap: 4px;
  padding: 4px;
  background: var(--card-raised);
  border: 1px solid var(--border);
  border-radius: 10px;
}
.filter-tabs button {
  padding: 6px 12px;
  color: var(--text-muted);
  background: transparent;
  border: 0;
  border-radius: 7px;
  cursor: pointer;
  font-size: 12px;
}
.filter-tabs button:hover {
  color: var(--text);
  background: var(--card);
}
.filter-tabs button.active {
  color: var(--accent-readable);
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
}
.icon-button {
  min-width: 40px;
  min-height: 40px;
}
.table-shell {
  overflow-x: auto;
  border: 1px solid var(--border);
  border-radius: 12px;
}
.items-table {
  width: 100%;
  border-collapse: collapse;
}
.items-table th,
.items-table td {
  padding: 10px 14px;
  text-align: left;
  vertical-align: top;
  border-bottom: 1px solid var(--border);
}
.items-table thead th {
  color: var(--text-subtle);
  background: var(--card-raised);
  font-size: 11px;
  font-weight: 650;
  white-space: nowrap;
}
.items-table tbody tr:last-child td {
  border-bottom: 0;
}
.items-table tbody tr:hover td {
  background: var(--card-raised);
}
.col-loc {
  width: 170px;
  min-width: 170px;
}
.col-actions {
  width: 96px;
  min-width: 96px;
}
.row-icon-btn {
  min-width: 30px;
  min-height: 30px;
  padding: 0 7px;
}
.cell-source {
  width: 34%;
  min-width: 220px;
}
.cell-target {
  min-width: 240px;
}
.item-text {
  display: block;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.55;
}
.item-reason {
  display: block;
  overflow: hidden;
  color: var(--danger);
  font-size: 12px;
  line-height: 1.5;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 320px;
}
.col-loc .mono {
  display: block;
  max-width: 165px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
}
.col-loc .status-pill {
  margin-top: 6px;
}
.row-actions {
  display: flex;
  gap: 6px;
  flex-wrap: nowrap;
}
.edit-textarea {
  width: 100%;
  min-height: 56px;
  resize: vertical;
  font-size: 12px;
  line-height: 1.5;
}
.table-loading {
  padding: 22px !important;
  text-align: center;
  font-size: 12px;
}
.table-loading .spin {
  display: inline-block;
  vertical-align: -2px;
  margin-right: 6px;
}
.pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 14px;
  font-size: 12px;
  flex-wrap: wrap;
}
.pager-buttons {
  display: flex;
  gap: 8px;
}
@media (max-width: 900px) {
  .stat-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  .export-form {
    grid-template-columns: minmax(0, 1fr);
  }
}
@media (max-width: 620px) {
  .stat-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .run-controls {
    width: 100%;
  }
}
</style>
