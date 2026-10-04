export type Language = 'zh-Hans' | 'zh-Hant' | 'en' | 'ja' | 'ko' | 'ru' | 'fr' | 'de'

export type ItemStatus = 'success' | 'failure' | 'waiting' | 'translating'

export type RunStatus = 'notstarted' | 'running' | 'paused' | 'completed'

export type ApiStyle = 'message' | 'response' | 'completion'

export type RunAction = 'start' | 'pause' | 'resume'

export type ExportMode = 'translated_only' | 'bilingual'

export interface ProjectObjectFile {
  type: 'file'
  path: string
}

export interface ProjectObjectFolder {
  type: 'folder'
  path: string
  file_paths: string[]
}

export type ProjectObject = ProjectObjectFile | ProjectObjectFolder

export interface ProjectConfig {
  task_limit: number
  retry_limit: number
  task_timeout_ms: number
  provider: string
}

export interface LlmConfig {
  api_style: ApiStyle
  base_url: string
  api_key: string
  model: string
  temperature: number | null
  top_p: number | null
  top_k: number | null
}

export interface LlmUsage {
  input: number
  output: number
}

export interface TokenRate {
  input: number
  output: number
}

export interface ItemCounts {
  total: number
  ignore: number
  success: number
  failure: number
  translating: number
  waiting: number
}

export interface ProjectInfo {
  original_name: string
  translate_name: string | null
  object: ProjectObject
  source_language: Language
  target_language: Language
  running_time_ms: number
  token_usage: LlmUsage
  run_status: RunStatus
}

export interface ProjectView {
  info: ProjectInfo
  counts: ItemCounts
  token_rate: TokenRate
  project_config: ProjectConfig
}

export interface ItemView {
  file: string
  line: number
  status: ItemStatus
  source: string
  target: string | null
  reason: string | null
}

export interface ItemsPage {
  total: number
  items: ItemView[]
}

export interface CountReply {
  retried: number
}

export interface ExportReply {
  files: number
}

export interface ProjectList {
  current: ProjectView | null
  recent: string[]
}

export interface FsEntry {
  name: string
  type: 'file' | 'dir'
}

export interface FsList {
  path: string
  parent: string | null
  entries: FsEntry[]
}

export interface CreateProjectBody {
  object: ProjectObject
  source: Language
  target: Language
  project_config: ProjectConfig
}

export interface ItemEvent {
  event: 'item'
  file: string
  line: number
  status: ItemStatus
  source: string
  target?: string | null
  reason?: string | null
}

export interface StatusEvent {
  event: 'status'
  status: RunStatus
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

const API_BASE = import.meta.env.VITE_API_BASE ?? ''

function parseJson<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T
  } catch {
    return null
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  if (init?.body) headers.set('content-type', 'application/json')
  const response = await fetch(`${API_BASE}${path}`, { ...init, headers })
  const body = await response.json().catch(() => null)
  if (!response.ok) {
    throw new ApiError(
      response.status,
      (body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : response.statusText) || response.statusText,
    )
  }
  return body as T
}

async function createProjectStream(
  body: CreateProjectBody,
  onProgress: (scanned: number, total: number) => void,
): Promise<ProjectView> {
  const response = await fetch(`${API_BASE}/api/projects`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!response.ok) {
    const errBody = await response.json().catch(() => null)
    throw new ApiError(
      response.status,
      errBody &&
        typeof errBody === 'object' &&
        'error' in errBody &&
        typeof errBody.error === 'string'
        ? errBody.error
        : response.statusText,
    )
  }
  if (!response.body) throw new ApiError(response.status, '后端未返回流式响应')
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    let index: number
    while ((index = buffer.indexOf('\n\n')) >= 0) {
      const block = buffer.slice(0, index)
      buffer = buffer.slice(index + 2)
      let eventName = ''
      let data = ''
      for (const line of block.split('\n')) {
        if (line.startsWith('event: ')) eventName = line.slice(7)
        else if (line.startsWith('data: ')) data = line.slice(6)
      }
      if (eventName === 'progress') {
        const parsed = parseJson<{ scanned: number; total: number }>(data)
        if (parsed) onProgress(parsed.scanned, parsed.total)
      } else if (eventName === 'done') {
        const view = parseJson<ProjectView>(data)
        if (view) return view
        throw new ApiError(response.status, '创建工程失败：响应解析失败')
      } else if (eventName === 'error') {
        const parsed = parseJson<{ error: string }>(data)
        throw new ApiError(response.status, parsed?.error ?? '创建工程失败')
      }
    }
  }
  throw new ApiError(response.status, '创建工程失败：响应意外结束')
}

function providerPath(name: string): string {
  return `/api/providers/${encodeURIComponent(name)}`
}

export const api = {
  projectList: () => request<ProjectList>('/api/projects'),
  createProject: (body: CreateProjectBody, onProgress: (scanned: number, total: number) => void) =>
    createProjectStream(body, onProgress),
  loadProject: (transPath: string) =>
    request<ProjectView>('/api/projects/load', {
      method: 'POST',
      body: JSON.stringify({ trans_path: transPath }),
    }),
  currentProject: () => request<ProjectView>('/api/projects/current'),
  run: (action: RunAction) =>
    request<ProjectView>('/api/projects/current/run', {
      method: 'POST',
      body: JSON.stringify({ action }),
    }),
  updateProjectConfig: (config: ProjectConfig) =>
    request<ProjectView>('/api/projects/current/config', {
      method: 'PUT',
      body: JSON.stringify(config),
    }),
  items: (query: { status?: ItemStatus | 'all'; offset?: number; limit?: number }) => {
    const params = new URLSearchParams()
    if (query.status && query.status !== 'all') params.set('status', query.status)
    if (query.offset !== undefined) params.set('offset', String(query.offset))
    if (query.limit !== undefined) params.set('limit', String(query.limit))
    const qs = params.toString()
    return request<ItemsPage>(`/api/projects/current/items${qs ? `?${qs}` : ''}`)
  },
  patchItem: (file: string, line: number, target: string) =>
    request<ProjectView>('/api/projects/current/items', {
      method: 'PATCH',
      body: JSON.stringify({ file, line, target }),
    }),
  resetItem: (file: string, line: number) =>
    request<ProjectView>('/api/projects/current/items/reset', {
      method: 'POST',
      body: JSON.stringify({ file, line }),
    }),
  retryAll: () => request<CountReply>('/api/projects/current/retry', { method: 'POST' }),
  retryItem: (file: string, line: number) =>
    request<CountReply>('/api/projects/current/retry/item', {
      method: 'POST',
      body: JSON.stringify({ file, line }),
    }),
  exportProject: (mode: ExportMode, outputDir: string) =>
    request<ExportReply>('/api/projects/current/export', {
      method: 'POST',
      body: JSON.stringify({ mode, output_dir: outputDir }),
    }),
  fsList: (path?: string) => {
    const qs = path ? `?path=${encodeURIComponent(path)}` : ''
    return request<FsList>(`/api/fs/list${qs}`)
  },
  providers: () => request<Record<string, LlmConfig>>('/api/providers'),
  upsertProvider: (name: string, config: LlmConfig) =>
    request<LlmConfig>('/api/providers', {
      method: 'POST',
      body: JSON.stringify({ name, config }),
    }),
  updateProvider: (name: string, config: LlmConfig) =>
    request<LlmConfig>(providerPath(name), {
      method: 'PUT',
      body: JSON.stringify({ config }),
    }),
  deleteProvider: (name: string) =>
    request<{ removed: string }>(providerPath(name), { method: 'DELETE' }),
  providerModels: (name: string) =>
    request<string[]>(`${providerPath(name)}/models`, { method: 'POST' }),
  providerModelsByConfig: (config: LlmConfig) =>
    request<string[]>('/api/providers/models', { method: 'POST', body: JSON.stringify(config) }),
}

export function eventUrl(): string {
  return `${API_BASE}/api/projects/current/events`
}
