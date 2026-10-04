<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { LuCheck, LuChevronDown, LuMoon, LuPalette, LuSun } from 'vue-icons-plus/lu'

import { accentOptions, flavorOptions, useThemeStore } from '@/stores/theme'

const theme = useThemeStore()
const { flavor, accent } = storeToRefs(theme)
const root = ref<HTMLElement | null>(null)
const trigger = ref<HTMLElement | null>(null)
const popover = ref<HTMLElement | null>(null)
const position = ref({ left: 8, top: 8 })
const open = ref(false)
const currentFlavor = computed(() => flavorOptions.find((option) => option.id === flavor.value)!)
const currentAccent = computed(() => accentOptions.find((option) => option.id === accent.value)!)

function closeOutside(event: PointerEvent) {
  const target = event.target as Node
  if (root.value?.contains(target) || popover.value?.contains(target)) return
  open.value = false
}

function updatePosition() {
  if (!trigger.value || !popover.value) return
  const anchor = trigger.value.getBoundingClientRect()
  const panel = popover.value.getBoundingClientRect()
  const left = Math.min(Math.max(anchor.left - 8, 8), window.innerWidth - panel.width - 8)
  const above = anchor.top - panel.height - 10
  const below = Math.min(anchor.bottom + 10, window.innerHeight - panel.height - 8)
  position.value = { left, top: above >= 8 ? above : Math.max(8, below) }
}

async function toggle() {
  open.value = !open.value
  if (!open.value) return
  await nextTick()
  updatePosition()
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') open.value = false
}

document.addEventListener('pointerdown', closeOutside)
document.addEventListener('keydown', handleKeydown)
window.addEventListener('resize', updatePosition)
window.addEventListener('scroll', updatePosition, true)
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', closeOutside)
  document.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('resize', updatePosition)
  window.removeEventListener('scroll', updatePosition, true)
})
</script>

<template>
  <div ref="root" class="appearance-picker" @keydown="handleKeydown">
    <button
      ref="trigger"
      class="appearance-trigger"
      type="button"
      :aria-expanded="open"
      aria-haspopup="dialog"
      :aria-label="`外观设置，当前 ${currentFlavor.label}，${currentAccent.label}`"
      @click="toggle"
    >
      <span class="appearance-icon">
        <LuSun v-if="!currentFlavor.dark" :size="15" />
        <LuMoon v-else :size="15" />
      </span>
      <span class="appearance-copy">
        <strong>{{ currentFlavor.label }}</strong>
        <small>{{ currentAccent.label }}</small>
      </span>
      <span class="current-accent" :style="{ '--swatch': `var(--ctp-${accent})` }"></span>
      <LuChevronDown class="appearance-chevron" :size="15" aria-hidden="true" />
    </button>

    <Teleport to="body">
      <Transition name="appearance-pop">
        <section
          v-if="open"
          ref="popover"
          class="appearance-popover"
          role="dialog"
          aria-label="外观设置"
          :style="{ left: `${position.left}px`, top: `${position.top}px` }"
        >
          <header>
            <LuPalette :size="16" />
            <div>
              <strong>外观设置</strong>
              <small>主题与强调色分别保存</small>
            </div>
          </header>

          <div class="appearance-section">
            <span class="section-label">主题</span>
            <div class="flavor-grid">
              <button
                v-for="option in flavorOptions"
                :key="option.id"
                type="button"
                :class="{ active: flavor === option.id }"
                :aria-pressed="flavor === option.id"
                @click="flavor = option.id"
              >
                <span>
                  <strong>{{ option.label }}</strong>
                  <small>{{ option.description }}</small>
                </span>
                <LuCheck v-if="flavor === option.id" :size="14" aria-hidden="true" />
              </button>
            </div>
          </div>

          <div class="appearance-section">
            <span class="section-label">主题色</span>
            <div class="accent-grid">
              <button
                v-for="option in accentOptions"
                :key="option.id"
                type="button"
                :class="{ active: accent === option.id }"
                :style="{ '--swatch': `var(--ctp-${option.id})` }"
                :title="option.label"
                :aria-label="option.label"
                :aria-pressed="accent === option.id"
                @click="accent = option.id"
              ></button>
            </div>
            <strong class="accent-name">{{ currentAccent.label }}</strong>
          </div>
        </section>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.appearance-picker {
  position: relative;
  min-width: 0;
}
.appearance-trigger {
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 9px;
  width: 100%;
  min-height: 42px;
  padding: 5px 8px 5px 6px;
  color: var(--text-muted);
  text-align: left;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 9px;
  cursor: pointer;
}
.appearance-trigger:hover,
.appearance-trigger[aria-expanded='true'] {
  color: var(--text);
  background: var(--card-raised);
  border-color: var(--border-strong);
}
.appearance-icon {
  display: grid;
  width: 30px;
  height: 30px;
  color: var(--accent-readable);
  background: color-mix(in srgb, var(--accent) 10%, var(--panel));
  border-radius: 7px;
  place-items: center;
}
.appearance-copy {
  min-width: 0;
}
.appearance-copy strong,
.appearance-copy small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.appearance-copy strong {
  font-size: 10px;
}
.appearance-copy small {
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 9px;
}
.current-accent {
  width: 12px;
  height: 12px;
  background: var(--swatch);
  border: 1px solid var(--border-strong);
  border-radius: 50%;
}
.appearance-chevron {
  color: var(--text-subtle);
  transition: transform 140ms ease;
}
.appearance-trigger[aria-expanded='true'] .appearance-chevron {
  transform: rotate(180deg);
}
.appearance-popover {
  position: fixed;
  z-index: 200;
  width: min(252px, calc(100vw - 16px));
  padding: 12px;
  background: var(--panel);
  border: 1px solid var(--border-strong);
  border-radius: 12px;
  box-shadow: 0 20px 55px -18px var(--shadow);
}
.appearance-popover header {
  display: flex;
  align-items: center;
  gap: 9px;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--border);
}
.appearance-popover header > svg {
  color: var(--accent-readable);
}
.appearance-popover header strong,
.appearance-popover header small {
  display: block;
}
.appearance-popover header strong {
  font-size: 11px;
}
.appearance-popover header small {
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 9px;
}
.appearance-section {
  margin-top: 11px;
}
.section-label {
  display: block;
  margin-bottom: 7px;
  color: var(--text-subtle);
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.flavor-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
}
.flavor-grid button {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 5px;
  min-width: 0;
  min-height: 44px;
  padding: 6px 7px;
  color: var(--text-muted);
  text-align: left;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 8px;
  cursor: pointer;
}
.flavor-grid button:hover,
.flavor-grid button.active {
  color: var(--text);
  border-color: color-mix(in srgb, var(--accent) 42%, var(--border));
}
.flavor-grid button.active {
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
}
.flavor-grid button > span {
  min-width: 0;
}
.flavor-grid strong,
.flavor-grid small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.flavor-grid strong {
  font-size: 10px;
}
.flavor-grid small {
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 8px;
}
.flavor-grid svg {
  flex: 0 0 auto;
  color: var(--accent-readable);
}
.accent-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 8px;
}
.accent-grid button {
  width: 20px;
  height: 20px;
  padding: 0;
  background: var(--swatch);
  border: 2px solid transparent;
  border-radius: 50%;
  cursor: pointer;
}
.accent-grid button:hover {
  transform: scale(1.12);
}
.accent-grid button.active {
  border-color: var(--panel);
  box-shadow: 0 0 0 2px var(--swatch);
}
.accent-name {
  display: block;
  margin-top: 8px;
  color: var(--text-muted);
  font-size: 9px;
  text-align: right;
}
.appearance-pop-enter-active,
.appearance-pop-leave-active {
  transition:
    opacity 120ms ease,
    transform 120ms ease;
  transform-origin: bottom left;
}
.appearance-pop-enter-from,
.appearance-pop-leave-to {
  opacity: 0;
  transform: translateY(5px) scale(0.98);
}
</style>
