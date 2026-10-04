<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { storeToRefs } from 'pinia'
import {
  LuBot,
  LuEye,
  LuEyeOff,
  LuKeyRound,
  LuLoader2,
  LuPencil,
  LuPlus,
  LuSave,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'

import { useAppToast } from '@/composables/useAppToast'
import { useWorkspaceStore } from '@/stores/workspace'
import type { ApiStyle, LlmConfig } from '@/api/client'

const workspace = useWorkspaceStore()
const { notify, confirmDelete } = useAppToast()
const { providers } = storeToRefs(workspace)

const editingName = ref<string | null>(null)
const draftName = ref('')
const draftConfig = reactive<LlmConfig>({
  api_style: 'completion',
  base_url: '',
  api_key: '',
  model: '',
  temperature: null,
  top_p: null,
  top_k: null,
})
const saving = ref(false)
const showKey = ref(false)
const fetchingModels = ref<string | null>(null)
const modelOpen = ref(false)
const modelFetching = ref(false)
const modelError = ref('')
const modelOptions = ref<string[]>([])
const modelActive = ref(-1)
const modelRoot = ref<HTMLElement | null>(null)

const apiStyleOptions: Array<{ value: ApiStyle; label: string }> = [
  { value: 'completion', label: 'OpenAI Chat Completions' },
  { value: 'response', label: 'OpenAI Responses' },
  { value: 'message', label: 'Anthropic Messages' },
]

const apiStyleLabels: Record<ApiStyle, string> = {
  completion: 'OpenAI Chat Completions',
  response: 'OpenAI Responses',
  message: 'Anthropic Messages',
}

const providerEntries = computed(() => Object.entries(providers.value))

function resetForm() {
  editingName.value = null
  draftName.value = ''
  draftConfig.api_style = 'completion'
  draftConfig.base_url = ''
  draftConfig.api_key = ''
  draftConfig.model = ''
  draftConfig.temperature = null
  draftConfig.top_p = null
  draftConfig.top_k = null
  showKey.value = false
  closeModelList()
}

async function openModelList() {
  if (modelOpen.value) return
  modelOpen.value = true
  modelActive.value = -1
  const base = draftConfig.base_url.trim()
  const key = draftConfig.api_key.trim()
  if (!base || !key) {
    modelError.value = ''
    modelOptions.value = []
    return
  }
  modelFetching.value = true
  modelError.value = ''
  try {
    modelOptions.value = await workspace.fetchModelsByConfig({
      api_style: draftConfig.api_style,
      base_url: base,
      api_key: key,
      model: draftConfig.model,
      temperature: null,
      top_p: null,
      top_k: null,
    })
  } catch (error) {
    modelError.value = error instanceof Error ? error.message : String(error)
    modelOptions.value = []
  } finally {
    modelFetching.value = false
  }
}

function closeModelList() {
  modelOpen.value = false
  modelActive.value = -1
}

function chooseModel(id: string) {
  draftConfig.model = id
  closeModelList()
}

function onModelKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    if (!modelOpen.value) {
      void openModelList()
      return
    }
    event.preventDefault()
    const count = modelOptions.value.length
    if (!count) return
    modelActive.value =
      modelActive.value < 0
        ? event.key === 'ArrowDown'
          ? 0
          : count - 1
        : (modelActive.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count
  } else if (event.key === 'Enter' && modelOpen.value && modelActive.value >= 0) {
    event.preventDefault()
    chooseModel(modelOptions.value[modelActive.value]!)
  } else if (event.key === 'Escape') {
    closeModelList()
  }
}

function handleOutside(event: PointerEvent) {
  if (modelRoot.value && !modelRoot.value.contains(event.target as Node)) closeModelList()
}

onMounted(() => document.addEventListener('pointerdown', handleOutside))
onBeforeUnmount(() => document.removeEventListener('pointerdown', handleOutside))

function beginCreate() {
  resetForm()
}

function beginEdit(name: string) {
  const config = providers.value[name]
  if (!config) return
  editingName.value = name
  draftName.value = name
  draftConfig.api_style = config.api_style
  draftConfig.base_url = config.base_url
  draftConfig.api_key = config.api_key
  draftConfig.model = config.model
  draftConfig.temperature = config.temperature
  draftConfig.top_p = config.top_p
  draftConfig.top_k = config.top_k
  showKey.value = false
  closeModelList()
}

function maskKey(key: string): string {
  if (key.length <= 8) return '••••••••'
  return `${key.slice(0, 4)}…${key.slice(-4)}`
}

function optionalNumber(value: number | '' | null): number | null {
  if (value === '' || value === null) return null
  return Number.isFinite(value) ? value : null
}

async function submitProvider() {
  const name = draftName.value.trim()
  if (!name) {
    notify('warning', '请填写配置名称', 'provider 名称用于工程中引用。')
    return
  }
  if (!draftConfig.base_url.trim()) {
    notify('warning', '请填写 Base URL', '例如 https://api.openai.com/v1。')
    return
  }
  if (!draftConfig.api_key.trim()) {
    notify('warning', '请填写 API Key', '翻译请求需要鉴权。')
    return
  }
  if (!draftConfig.model.trim()) {
    notify('warning', '请填写模型 ID', '例如 gpt-4o-mini。')
    return
  }
  saving.value = true
  try {
    await workspace.saveProvider(
      name,
      {
        api_style: draftConfig.api_style,
        base_url: draftConfig.base_url.trim(),
        api_key: draftConfig.api_key.trim(),
        model: draftConfig.model.trim(),
        temperature: optionalNumber(draftConfig.temperature),
        top_p: optionalNumber(draftConfig.top_p),
        top_k: optionalNumber(draftConfig.top_k),
      },
      editingName.value === null,
    )
    notify('success', editingName.value === null ? '模型接口已添加' : '模型接口已更新', name)
    resetForm()
  } catch (error) {
    notify('error', '保存失败', error instanceof Error ? error.message : String(error))
  } finally {
    saving.value = false
  }
}

async function removeProvider(name: string) {
  const confirmed = await confirmDelete({
    id: name,
    title: `删除模型接口“${name}”？`,
    description: '删除后需要重新配置才能使用该 provider。',
  })
  if (!confirmed) return
  try {
    await workspace.deleteProvider(name)
    if (editingName.value === name) resetForm()
    notify('success', '模型接口已删除', name)
  } catch (error) {
    notify('error', '删除失败', error instanceof Error ? error.message : String(error))
  }
}

async function fetchModels(name: string) {
  fetchingModels.value = name
  try {
    const models = await workspace.fetchModels(name)
    const preview = models.slice(0, 5).join('、')
    notify(
      models.length ? 'success' : 'warning',
      '模型列表已获取',
      models.length
        ? `共 ${models.length} 个：${preview}${models.length > 5 ? '…' : ''}`
        : '服务端未返回可用模型。',
    )
  } catch (error) {
    notify('error', '获取模型列表失败', error instanceof Error ? error.message : String(error))
  } finally {
    fetchingModels.value = null
  }
}
</script>

<template>
  <header class="page-heading">
    <div>
      <p class="eyebrow">Providers</p>
      <h1>模型接口</h1>
    </div>
  </header>

  <div class="provider-columns">
    <section class="panel" aria-labelledby="provider-form-heading">
      <div class="panel-heading">
        <div>
          <h2 id="provider-form-heading">{{ editingName ? '编辑模型接口' : '新建模型接口' }}</h2>
          <p>配置 LLM 服务，供翻译工程引用。</p>
        </div>
      </div>

      <div class="form-grid">
        <div class="field span-2">
          <label class="field-label" for="provider-name">名称</label>
          <input
            id="provider-name"
            v-model="draftName"
            type="text"
            placeholder="例如 claude / deepseek"
            :disabled="editingName !== null"
            autocomplete="off"
            spellcheck="false"
          />
          <small v-if="editingName" class="field-hint">名称创建后不可修改。</small>
        </div>

        <div class="field span-2">
          <label class="field-label" for="api-style">接口协议</label>
          <select id="api-style" v-model="draftConfig.api_style">
            <option v-for="option in apiStyleOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
        </div>

        <div class="field span-2">
          <label class="field-label" for="base-url">Base URL</label>
          <input
            id="base-url"
            v-model="draftConfig.base_url"
            type="text"
            placeholder="https://api.openai.com/v1"
            autocomplete="off"
            spellcheck="false"
          />
        </div>

        <div class="field span-2">
          <label class="field-label" for="api-key">API Key</label>
          <div class="key-input">
            <LuKeyRound :size="14" class="key-icon" aria-hidden="true" />
            <input
              id="api-key"
              v-model="draftConfig.api_key"
              :type="showKey ? 'text' : 'password'"
              placeholder="sk-…"
              autocomplete="off"
              spellcheck="false"
            />
            <button
              class="button ghost icon-button"
              type="button"
              :aria-label="showKey ? '隐藏 API Key' : '显示 API Key'"
              :title="showKey ? '隐藏' : '显示'"
              @click="showKey = !showKey"
            >
              <LuEyeOff v-if="showKey" :size="14" aria-hidden="true" />
              <LuEye v-else :size="14" aria-hidden="true" />
            </button>
          </div>
        </div>

        <div class="field span-2">
          <label class="field-label" for="model-id">模型 ID</label>
          <div ref="modelRoot" class="model-combobox">
            <input
              id="model-id"
              v-model="draftConfig.model"
              type="text"
              placeholder="gpt-4o-mini"
              autocomplete="off"
              spellcheck="false"
              role="combobox"
              :aria-expanded="modelOpen"
              aria-controls="model-options"
              aria-autocomplete="list"
              @focus="openModelList"
              @keydown="onModelKeydown"
            />
            <span v-if="modelFetching" class="model-spinner" aria-hidden="true"
              ><LuLoader2 :size="14" class="spin"
            /></span>
            <Transition name="model-pop">
              <ul v-if="modelOpen" id="model-options" class="model-options" role="listbox">
                <li v-if="modelFetching" class="model-option muted">正在获取模型…</li>
                <li v-else-if="modelError" class="model-option error">{{ modelError }}</li>
                <li
                  v-else-if="!draftConfig.base_url.trim() || !draftConfig.api_key.trim()"
                  class="model-option muted"
                >
                  填写 Base URL 和 API Key 后可获取模型列表。
                </li>
                <li v-else-if="!modelOptions.length" class="model-option muted">没有可用模型。</li>
                <li v-else class="model-option">
                  <button
                    v-for="(id, index) in modelOptions"
                    :key="id"
                    class="mono"
                    type="button"
                    role="option"
                    :aria-selected="index === modelActive"
                    :class="{ active: index === modelActive }"
                    @click="chooseModel(id)"
                    @mouseenter="modelActive = index"
                  >
                    {{ id }}
                  </button>
                </li>
              </ul>
            </Transition>
          </div>
        </div>

        <div class="field">
          <label class="field-label" for="temperature">Temperature</label>
          <input
            id="temperature"
            v-model.number="draftConfig.temperature"
            type="number"
            min="0"
            max="2"
            step="0.1"
            placeholder="留空使用默认"
          />
        </div>
        <div class="field">
          <label class="field-label" for="top-p">Top P</label>
          <input
            id="top-p"
            v-model.number="draftConfig.top_p"
            type="number"
            min="0"
            max="1"
            step="0.05"
            placeholder="留空使用默认"
          />
        </div>
        <div class="field span-2">
          <label class="field-label" for="top-k">Top K</label>
          <input
            id="top-k"
            v-model.number="draftConfig.top_k"
            type="number"
            min="1"
            max="100"
            step="1"
            placeholder="仅 Anthropic Messages 支持"
          />
        </div>
      </div>

      <div class="panel-actions submit-row">
        <button class="button primary" type="button" :disabled="saving" @click="submitProvider">
          <span v-if="saving" class="spin"><LuLoader2 :size="15" aria-hidden="true" /></span>
          <LuSave v-else :size="15" aria-hidden="true" />
          {{ editingName ? '保存修改' : '添加接口' }}
        </button>
        <button v-if="editingName" class="button ghost" type="button" @click="resetForm">
          <LuX :size="15" aria-hidden="true" />
          取消编辑
        </button>
      </div>
    </section>

    <section class="panel" aria-labelledby="provider-list-heading">
      <div class="panel-heading">
        <div>
          <h2 id="provider-list-heading">已配置接口</h2>
          <p>共 {{ providerEntries.length }} 个 provider。</p>
        </div>
        <button class="button secondary" type="button" @click="beginCreate">
          <LuPlus :size="15" aria-hidden="true" />
          新建
        </button>
      </div>

      <div v-if="providerEntries.length" class="provider-list">
        <div v-for="[name, config] in providerEntries" :key="name" class="provider-row">
          <span class="row-icon" aria-hidden="true">
            <LuBot :size="15" />
          </span>
          <div class="provider-copy">
            <strong>{{ name }}</strong>
            <small>{{ apiStyleLabels[config.api_style] }} · {{ config.model }}</small>
            <small class="mono">{{ config.base_url }}</small>
            <small class="key-mask">{{ maskKey(config.api_key) }}</small>
          </div>
          <div class="provider-actions">
            <button
              class="button ghost small"
              type="button"
              :disabled="fetchingModels === name"
              @click="fetchModels(name)"
            >
              <span v-if="fetchingModels === name" class="spin"
                ><LuLoader2 :size="13" aria-hidden="true"
              /></span>
              获取模型
            </button>
            <button class="button ghost small" type="button" @click="beginEdit(name)">
              <LuPencil :size="13" aria-hidden="true" />
              编辑
            </button>
            <button class="button ghost small danger" type="button" @click="removeProvider(name)">
              <LuTrash2 :size="13" aria-hidden="true" />
              删除
            </button>
          </div>
        </div>
      </div>
      <p v-else class="empty-inline">尚未配置模型接口，请先在左侧新建。</p>
    </section>
  </div>
</template>

<style scoped>
.provider-columns {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 20px;
  align-items: start;
}
.submit-row {
  margin-top: 20px;
}
.key-input {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
  padding: 0 6px 0 12px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 9px;
  transition:
    border-color 140ms ease,
    box-shadow 140ms ease;
}
.key-input:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--focus-ring);
}
.model-combobox {
  position: relative;
}
.model-spinner {
  position: absolute;
  right: 10px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-subtle);
}
.model-options {
  position: absolute;
  z-index: 100;
  top: calc(100% + 5px);
  right: 0;
  left: 0;
  max-height: 220px;
  margin: 0;
  padding: 6px;
  overflow-y: auto;
  list-style: none;
  background: var(--panel);
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  box-shadow: 0 18px 48px -18px var(--shadow);
}
.model-option {
  padding: 8px 10px;
  color: var(--text-muted);
  font-size: 12px;
}
.model-option.error {
  color: var(--danger);
  word-break: break-word;
}
.model-option.muted {
  color: var(--text-subtle);
}
.model-option button {
  display: block;
  width: 100%;
  padding: 8px 10px;
  color: var(--text-muted);
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: 7px;
  font-size: 12px;
  cursor: pointer;
}
.model-option button:hover,
.model-option button.active {
  color: var(--text);
  background: var(--card-raised);
}
.model-pop-enter-active,
.model-pop-leave-active {
  transition:
    opacity 120ms ease,
    transform 120ms ease;
  transform-origin: top;
}
.model-pop-enter-from,
.model-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
.key-icon {
  flex: 0 0 auto;
  color: var(--text-subtle);
}
.key-input input {
  flex: 1 1 auto;
  min-width: 0;
  border: 0 !important;
  box-shadow: none !important;
  background: transparent !important;
}
.key-input input:focus-visible {
  outline: none;
}
.icon-button {
  min-width: 32px;
  min-height: 32px;
  padding: 0 8px;
}
.provider-list {
  display: grid;
  gap: 8px;
}
.provider-row {
  display: flex;
  align-items: flex-start;
  gap: 11px;
  padding: 11px;
  background: var(--card-raised);
  border: 1px solid var(--border);
  border-radius: 11px;
}
.provider-copy {
  display: grid;
  gap: 3px;
  min-width: 0;
  flex: 1 1 auto;
}
.provider-copy strong {
  font-size: 13px;
}
.provider-copy small {
  min-width: 0;
  overflow: hidden;
  color: var(--text-muted);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.provider-copy .mono {
  font-size: 10px;
}
.key-mask {
  color: var(--text-subtle) !important;
}
.provider-actions {
  display: flex;
  gap: 6px;
  flex: 0 0 auto;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.empty-inline {
  margin: 0;
  color: var(--text-subtle);
  font-size: 12px;
}
@media (max-width: 1050px) {
  .provider-columns {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
