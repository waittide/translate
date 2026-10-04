<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import { LuArrowRight, LuFolder, LuFileText, LuFolderOpen, LuPlus } from 'vue-icons-plus/lu'

import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import FilePicker from '@/components/FilePicker.vue'
import ProgressBar from '@/components/ProgressBar.vue'
import { useAppToast } from '@/composables/useAppToast'
import { useWorkspaceStore } from '@/stores/workspace'
import type { Language } from '@/api/client'

const router = useRouter()
const workspace = useWorkspaceStore()
const { notify } = useAppToast()
const { project, recent, providers, counts, loading } = storeToRefs(workspace)

const objectType = ref<'file' | 'folder'>('file')
const path = ref('')
const sourceLanguage = ref<Language>('zh-Hans')
const targetLanguage = ref<Language>('en')
const provider = ref('')
const taskLimit = ref(4)
const retryLimit = ref(3)
const timeoutSec = ref(300)
const busy = ref(false)
const loadingPath = ref('')
const loadPath = ref('')
const pickerOpen = ref(false)
const createScanned = ref(0)
const createTotal = ref(0)

const languageOptions: SelectOption[] = [
  { value: 'zh-Hans', label: '简体中文' },
  { value: 'zh-Hant', label: '繁体中文' },
  { value: 'en', label: '英语' },
  { value: 'ja', label: '日语' },
  { value: 'ko', label: '韩语' },
  { value: 'ru', label: '俄语' },
  { value: 'fr', label: '法语' },
  { value: 'de', label: '德语' },
]

const providerOptions = computed<SelectOption[]>(() =>
  Object.keys(providers.value).map((name) => ({
    value: name,
    label: name,
    description: providers.value[name]?.model ?? undefined,
  })),
)

const createPct = computed(() => {
  if (createTotal.value === 0) return 0
  return Math.round((createScanned.value / createTotal.value) * 100)
})

const noProviders = computed(() => Object.keys(providers.value).length === 0)

onMounted(async () => {
  if (project.value) await workspace.loadProjects()
})

function objectLabel(type: 'file' | 'folder') {
  return type === 'file' ? '单个文件' : '文件夹'
}

function onPickerSelect(picked: string) {
  path.value = picked
  pickerOpen.value = false
}

async function submitCreate() {
  const trimmed = path.value.trim()
  if (!trimmed) {
    notify(
      'warning',
      '请填写路径',
      objectType.value === 'file' ? '需要指定 txt 文件路径。' : '需要指定文件夹路径。',
    )
    return
  }
  if (!provider.value) {
    notify('warning', '请先配置模型接口', '创建工程需要选择一个已配置的 provider。')
    return
  }
  busy.value = true
  createScanned.value = 0
  createTotal.value = 0
  try {
    const created = await workspace.createProject(
      {
        object:
          objectType.value === 'file'
            ? { type: 'file', path: trimmed }
            : { type: 'folder', path: trimmed, file_paths: [] },
        source: sourceLanguage.value,
        target: targetLanguage.value,
        project_config: {
          task_limit: Math.max(1, Math.floor(taskLimit.value)),
          retry_limit: Math.max(0, Math.floor(retryLimit.value)),
          task_timeout_ms: Math.max(1000, Math.floor(timeoutSec.value) * 1000),
          provider: provider.value,
        },
      },
      (scanned, total) => {
        createScanned.value = scanned
        createTotal.value = total
      },
    )
    notify(
      'success',
      '工程已创建',
      `${created.info.original_name}（共 ${created.counts.total} 条）`,
    )
    await router.push('/workbench')
  } catch (error) {
    notify('error', '创建工程失败', error instanceof Error ? error.message : String(error))
  } finally {
    busy.value = false
  }
}

async function submitLoad() {
  const trimmed = loadPath.value.trim()
  if (!trimmed) {
    notify('warning', '请填写路径', '需要指定 `.trans` 工程文件的路径。')
    return
  }
  await loadTrans(trimmed)
}

async function loadTrans(transPath: string) {
  loadingPath.value = transPath
  try {
    const loaded = await workspace.loadProject(transPath)
    notify('success', '工程已载入', `${loaded.info.original_name}（共 ${loaded.counts.total} 条）`)
    await router.push('/workbench')
  } catch (error) {
    notify('error', '载入工程失败', error instanceof Error ? error.message : String(error))
  } finally {
    loadingPath.value = ''
  }
}
</script>

<template>
  <header class="page-heading">
    <div>
      <p class="eyebrow">Projects</p>
      <h1>项目管理</h1>
    </div>
  </header>

  <section v-if="project" class="panel current-project">
    <div class="current-project-copy">
      <span class="file-icon" aria-hidden="true">
        <LuFolderOpen :size="18" />
      </span>
      <div class="current-project-title">
        <strong>{{ project.info.translate_name ?? project.info.original_name }}</strong>
        <small v-if="project.info.translate_name">{{ project.info.original_name }}</small>
      </div>
      <div class="current-project-meta">
        <span>{{ project.info.source_language }} → {{ project.info.target_language }}</span>
        <span class="mono">{{ project.info.object.path }}</span>
      </div>
      <div class="current-project-stats">
        <span
          >成功 <strong class="success-text">{{ counts?.success ?? 0 }}</strong></span
        >
        <span
          >失败 <strong class="danger-text">{{ counts?.failure ?? 0 }}</strong></span
        >
        <span
          >等待 <strong>{{ counts?.waiting ?? 0 }}</strong></span
        >
      </div>
    </div>
    <div class="current-project-actions">
      <span class="status-pill" :class="project.info.run_status">{{
        project.info.run_status
      }}</span>
      <RouterLink class="button primary" :to="{ name: 'workbench' }">
        <LuArrowRight :size="15" aria-hidden="true" />
        打开工作台
      </RouterLink>
    </div>
  </section>

  <div class="project-columns">
    <section class="panel" aria-labelledby="create-heading">
      <div class="panel-heading">
        <div>
          <h2 id="create-heading">新建工程</h2>
          <p>指定本地 txt 文件或文件夹，扫描后生成待翻译条目。</p>
        </div>
      </div>

      <div class="form-grid">
        <div class="field span-2">
          <span class="field-label">翻译对象</span>
          <div class="segmented" role="group" aria-label="翻译对象类型">
            <button
              type="button"
              :class="{ active: objectType === 'file' }"
              :aria-pressed="objectType === 'file'"
              @click="objectType = 'file'"
            >
              <LuFileText :size="15" aria-hidden="true" />
              单个文件
            </button>
            <button
              type="button"
              :class="{ active: objectType === 'folder' }"
              :aria-pressed="objectType === 'folder'"
              @click="objectType = 'folder'"
            >
              <LuFolder :size="15" aria-hidden="true" />
              文件夹
            </button>
          </div>
        </div>

        <div class="field span-2">
          <label class="field-label" for="object-path">
            {{ objectType === 'file' ? '文件路径' : '文件夹路径' }}
          </label>
          <div class="path-input-row">
            <input
              id="object-path"
              v-model="path"
              type="text"
              :placeholder="objectType === 'file' ? '/path/to/novel.txt' : '/path/to/novels'"
              autocomplete="off"
              spellcheck="false"
            />
            <button class="button secondary browse-btn" type="button" @click="pickerOpen = true">
              <LuFolderOpen :size="15" aria-hidden="true" />
              浏览
            </button>
          </div>
          <small class="field-hint">
            {{ objectType === 'file' ? '单个 txt 文件' : '递归扫描该目录下全部 txt 文件' }}
          </small>
        </div>

        <div class="field">
          <span class="field-label" id="source-lang-label">原文语言</span>
          <AppSelect v-model="sourceLanguage" :options="languageOptions" searchable />
        </div>
        <div class="field">
          <span class="field-label" id="target-lang-label">译文语言</span>
          <AppSelect v-model="targetLanguage" :options="languageOptions" searchable />
        </div>

        <div class="field span-2">
          <span class="field-label" id="provider-label">模型接口</span>
          <AppSelect
            v-model="provider"
            :options="providerOptions"
            :placeholder="noProviders ? '尚未配置任何 provider' : '请选择 provider'"
            searchable
          />
          <small v-if="noProviders" class="field-hint">
            还没有可用的模型接口，
            <RouterLink :to="{ name: 'providers' }">前往模型接口配置</RouterLink>
            。
          </small>
        </div>

        <div class="field">
          <label class="field-label" for="task-limit">并发任务数</label>
          <input id="task-limit" v-model.number="taskLimit" type="number" min="1" max="64" />
        </div>
        <div class="field">
          <label class="field-label" for="retry-limit">失败重试次数</label>
          <input id="retry-limit" v-model.number="retryLimit" type="number" min="0" max="10" />
        </div>
        <div class="field span-2">
          <label class="field-label" for="timeout">单次请求超时（秒）</label>
          <input id="timeout" v-model.number="timeoutSec" type="number" min="1" max="3600" />
        </div>
      </div>

      <div class="panel-actions submit-row">
        <button class="button primary" type="button" :disabled="busy" @click="submitCreate">
          <LuPlus v-if="!busy" :size="15" aria-hidden="true" />
          <span v-else class="spin"><LuLoader2 :size="15" aria-hidden="true" /></span>
          创建工程
        </button>
      </div>
      <div v-if="busy" class="create-progress">
        <ProgressBar :value="createPct" />
        <p class="progress-label">已扫描 {{ createScanned }} / {{ createTotal }} 行</p>
      </div>
    </section>

    <section class="panel" aria-labelledby="load-heading">
      <div class="panel-heading">
        <div>
          <h2 id="load-heading">载入工程</h2>
          <p>输入 `.trans` 工程文件路径，或从最近打开中快速选择。</p>
        </div>
      </div>

      <div class="load-form">
        <div class="field load-path">
          <label class="field-label" for="load-path">工程文件路径</label>
          <input
            id="load-path"
            v-model="loadPath"
            type="text"
            placeholder="/path/to/novel.trans"
            autocomplete="off"
            spellcheck="false"
            @keydown.enter="submitLoad"
          />
        </div>
        <button
          class="button secondary load-submit"
          type="button"
          :disabled="loadingPath !== ''"
          @click="submitLoad"
        >
          <span v-if="loadingPath !== ''" class="spin"
            ><LuLoader2 :size="14" aria-hidden="true"
          /></span>
          载入
        </button>
      </div>

      <h3 v-if="recent.length" class="recent-heading">最近打开</h3>
      <div v-if="recent.length" class="recent-list">
        <div v-for="transPath in recent" :key="transPath" class="recent-row">
          <span class="row-icon" aria-hidden="true">
            <LuFileText :size="15" />
          </span>
          <span class="mono">{{ transPath }}</span>
          <button
            class="button secondary"
            type="button"
            :disabled="loadingPath === transPath"
            @click="loadTrans(transPath)"
          >
            <span v-if="loadingPath === transPath" class="spin"
              ><LuLoader2 :size="14" aria-hidden="true"
            /></span>
            载入
          </button>
        </div>
      </div>
      <p v-else class="empty-inline">
        {{ loading ? '正在读取最近工程…' : '暂无最近工程。' }}
      </p>
    </section>
  </div>

  <FilePicker
    v-if="pickerOpen"
    :mode="objectType"
    @select="onPickerSelect"
    @close="pickerOpen = false"
  />
</template>

<style scoped>
.current-project {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}
.current-project-copy {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
}
.current-project-title {
  min-width: 0;
}
.current-project-title strong,
.current-project-title small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.current-project-title strong {
  font-size: 15px;
}
.current-project-title small {
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 11px;
}
.current-project-meta {
  display: grid;
  gap: 3px;
  min-width: 0;
  color: var(--text-muted);
  font-size: 11px;
}
.current-project-meta span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.current-project-stats {
  display: flex;
  gap: 14px;
  color: var(--text-subtle);
  font-size: 11px;
}
.current-project-stats strong {
  margin-left: 3px;
  font-size: 13px;
}
.current-project-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 0 0 auto;
}
.project-columns {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
  gap: 20px;
  align-items: start;
}
.submit-row {
  margin-top: 20px;
}
.create-progress {
  display: grid;
  gap: 8px;
  margin-top: 16px;
}
.progress-label {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}
.recent-list {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 8px;
}
.load-form {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
  align-items: end;
}
.path-input-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
}
.browse-btn {
  min-height: 40px;
}
.load-submit {
  min-height: 40px;
}
.recent-heading {
  margin: 18px 0 8px;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 650;
}
.recent-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  background: var(--card-raised);
  border: 1px solid var(--border);
  border-radius: 10px;
}
.recent-row .mono {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
}
.empty-inline {
  margin: 0;
  color: var(--text-subtle);
  font-size: 12px;
}
@media (max-width: 1050px) {
  .project-columns {
    grid-template-columns: minmax(0, 1fr);
  }
}
@media (max-width: 900px) {
  .current-project {
    flex-direction: column;
    align-items: flex-start;
  }
  .current-project-copy {
    flex-wrap: wrap;
  }
}
</style>
