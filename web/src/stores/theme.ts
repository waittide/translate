import { ref, watch } from 'vue'
import { defineStore } from 'pinia'

export const flavorOptions = [
  { id: 'latte', label: 'Latte', description: '明亮、轻盈', dark: false },
  { id: 'frappe', label: 'Frappé', description: '柔和、低对比', dark: true },
  { id: 'macchiato', label: 'Macchiato', description: '平衡、沉静', dark: true },
  { id: 'mocha', label: 'Mocha', description: '深邃、高对比', dark: true },
] as const

export const accentOptions = [
  { id: 'rosewater', label: 'Rosewater' },
  { id: 'flamingo', label: 'Flamingo' },
  { id: 'pink', label: 'Pink' },
  { id: 'mauve', label: 'Mauve' },
  { id: 'red', label: 'Red' },
  { id: 'maroon', label: 'Maroon' },
  { id: 'peach', label: 'Peach' },
  { id: 'yellow', label: 'Yellow' },
  { id: 'green', label: 'Green' },
  { id: 'teal', label: 'Teal' },
  { id: 'sky', label: 'Sky' },
  { id: 'sapphire', label: 'Sapphire' },
  { id: 'blue', label: 'Blue' },
  { id: 'lavender', label: 'Lavender' },
] as const

export type Flavor = (typeof flavorOptions)[number]['id']
export type Accent = (typeof accentOptions)[number]['id']

const flavorStorageKey = 'catppuccin-flavor'
const accentStorageKey = 'catppuccin-accent'

function readStoredValue<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = localStorage.getItem(key)
    return value && allowed.includes(value as T) ? (value as T) : fallback
  } catch {
    return fallback
  }
}

function storeValue(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // The theme still works when storage is unavailable.
  }
}

export const useThemeStore = defineStore('theme', () => {
  const flavor = ref<Flavor>(
    readStoredValue(
      flavorStorageKey,
      flavorOptions.map((option) => option.id),
      'mocha',
    ),
  )
  const accent = ref<Accent>(
    readStoredValue(
      accentStorageKey,
      accentOptions.map((option) => option.id),
      'mauve',
    ),
  )

  watch(
    [flavor, accent],
    ([nextFlavor, nextAccent]) => {
      const root = document.documentElement
      root.dataset.theme = nextFlavor
      root.dataset.accent = nextAccent
      root.style.colorScheme = nextFlavor === 'latte' ? 'light' : 'dark'
      storeValue(flavorStorageKey, nextFlavor)
      storeValue(accentStorageKey, nextAccent)
    },
    { immediate: true },
  )

  return { flavor, accent }
})
