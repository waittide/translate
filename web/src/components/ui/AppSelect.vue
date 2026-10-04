<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { LuCheck, LuChevronDown, LuSearch } from 'vue-icons-plus/lu'

export interface SelectOption {
  value: string
  label: string
  description?: string
}

const props = withDefaults(
  defineProps<{
    options: SelectOption[]
    placeholder?: string
    searchable?: boolean
    disabled?: boolean
  }>(),
  {
    placeholder: '请选择',
    searchable: false,
    disabled: false,
  },
)

const modelValue = defineModel<string>({ required: true })
const root = ref<HTMLElement | null>(null)
const searchInput = ref<HTMLInputElement | null>(null)
const open = ref(false)
const query = ref('')
const activeIndex = ref(-1)

const selected = computed(
  () => props.options.find((option) => option.value === modelValue.value) ?? null,
)
const filtered = computed(() => {
  const needle = query.value.trim().toLowerCase()
  if (!needle) return props.options
  return props.options.filter(
    (option) =>
      option.label.toLowerCase().includes(needle) ||
      option.value.toLowerCase().includes(needle) ||
      option.description?.toLowerCase().includes(needle),
  )
})

watch(open, async (isOpen) => {
  if (!isOpen) {
    query.value = ''
    return
  }
  activeIndex.value = -1
  await nextTick()
  if (props.searchable) searchInput.value?.focus()
})

watch(query, () => {
  activeIndex.value = -1
})

function toggle() {
  if (props.disabled) return
  open.value = !open.value
}

function choose(option: SelectOption) {
  modelValue.value = option.value
  open.value = false
}

function move(step: number) {
  if (!open.value) {
    open.value = true
    return
  }
  if (!filtered.value.length) return
  activeIndex.value =
    activeIndex.value < 0
      ? step > 0
        ? 0
        : filtered.value.length - 1
      : (activeIndex.value + step + filtered.value.length) % filtered.value.length
  nextTick(() =>
    root.value
      ?.querySelector<HTMLElement>(`[data-index="${activeIndex.value}"]`)
      ?.scrollIntoView({ block: 'nearest' }),
  )
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault()
    move(event.key === 'ArrowDown' ? 1 : -1)
  } else if (event.key === 'Enter' && open.value) {
    event.preventDefault()
    const option = filtered.value[activeIndex.value]
    if (option) choose(option)
  } else if (event.key === 'Escape') {
    open.value = false
  }
}

function handleOutside(event: PointerEvent) {
  if (root.value && !root.value.contains(event.target as Node)) open.value = false
}

document.addEventListener('pointerdown', handleOutside)
onBeforeUnmount(() => document.removeEventListener('pointerdown', handleOutside))
</script>

<template>
  <div ref="root" class="app-select" :class="{ open, disabled }" @keydown="handleKeydown">
    <button
      class="select-trigger"
      type="button"
      :disabled="disabled"
      :aria-expanded="open"
      aria-haspopup="listbox"
      @click="toggle"
    >
      <span class="select-value">
        <strong>{{ selected?.label ?? placeholder }}</strong>
        <small v-if="selected?.description">{{ selected.description }}</small>
      </span>
      <LuChevronDown class="select-chevron" :size="17" aria-hidden="true" />
    </button>

    <Transition name="select-pop">
      <div v-if="open" class="select-popover">
        <label v-if="searchable" class="select-search">
          <LuSearch :size="15" aria-hidden="true" />
          <input ref="searchInput" v-model="query" type="search" placeholder="搜索选项" />
        </label>
        <div class="select-options" role="listbox">
          <button
            v-for="(option, index) in filtered"
            :key="option.value"
            class="select-option"
            :class="{ selected: option.value === modelValue, active: index === activeIndex }"
            :data-index="index"
            type="button"
            role="option"
            :aria-selected="option.value === modelValue"
            @click="choose(option)"
          >
            <span>
              <strong>{{ option.label }}</strong>
              <small v-if="option.description">{{ option.description }}</small>
            </span>
            <LuCheck v-if="option.value === modelValue" :size="16" aria-hidden="true" />
          </button>
          <p v-if="!filtered.length" class="select-empty">没有匹配选项</p>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.app-select {
  position: relative;
  width: 100%;
  min-width: 0;
}

.select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  min-height: 42px;
  padding: 7px 10px 7px 12px;
  color: var(--text);
  text-align: left;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 9px;
  cursor: pointer;
  transition:
    border-color 140ms ease,
    box-shadow 140ms ease,
    background 140ms ease;
}

.select-trigger:hover:not(:disabled),
.open .select-trigger {
  background: color-mix(in srgb, var(--accent) 4%, var(--card));
  border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  box-shadow: 0 0 0 2px var(--focus-ring);
}

.select-trigger:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}
.select-value {
  min-width: 0;
}
.select-value strong,
.select-value small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.select-value strong {
  font-size: 12px;
  font-weight: 650;
}
.select-value small {
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 10px;
}
.select-chevron {
  flex: 0 0 auto;
  color: var(--text-subtle);
  transition: transform 140ms ease;
}
.open .select-chevron {
  color: var(--accent-readable);
  transform: rotate(180deg);
}

.select-popover {
  position: absolute;
  z-index: 100;
  top: calc(100% + 7px);
  right: 0;
  left: 0;
  padding: 6px;
  overflow: hidden;
  background: var(--panel);
  border: 1px solid var(--border-strong);
  border-radius: 11px;
  box-shadow: 0 18px 48px -18px var(--shadow);
}

.select-search {
  display: flex !important;
  align-items: center;
  gap: 8px !important;
  min-width: 0;
  padding: 0 9px;
  margin-bottom: 5px;
  color: var(--text-subtle) !important;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 8px;
}
.select-search input {
  min-width: 0;
  min-height: 34px !important;
  padding: 0 !important;
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
}
.select-search:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--focus-ring);
}
.select-search input:focus-visible {
  outline: none !important;
}
.select-options {
  max-height: 240px;
  overflow-y: auto;
}
.select-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  padding: 9px 10px;
  color: var(--text-muted);
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: 7px;
  cursor: pointer;
}
.select-option:hover,
.select-option.active {
  color: var(--text);
  background: var(--card-raised);
}
.select-option.selected {
  color: var(--accent-readable);
  background: color-mix(in srgb, var(--accent) 9%, var(--card));
}
.select-option > span {
  min-width: 0;
}
.select-option strong,
.select-option small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.select-option strong {
  font-size: 12px;
  font-weight: 600;
}
.select-option small {
  margin-top: 3px;
  color: var(--text-subtle);
  font-size: 10px;
}
.select-option > svg {
  flex: 0 0 auto;
}
.select-empty {
  padding: 14px 10px;
  margin: 0;
  color: var(--text-subtle);
  text-align: center;
  font-size: 11px;
}
.select-pop-enter-active,
.select-pop-leave-active {
  transition:
    opacity 120ms ease,
    transform 120ms ease;
  transform-origin: top;
}
.select-pop-enter-from,
.select-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px) scale(0.98);
}
</style>
