<template>
  <div
    ref="rootRef"
    :class="['fluent-select-wrap', { 'is-open': isOpen, 'is-disabled': disabled }]"
  >
    <button
      type="button"
      class="fluent-select-trigger"
      :disabled="disabled"
      @click="toggleOpen"
    >
      <span class="fluent-select-label">{{ currentLabel }}</span>
      <SvgIcon
        :name="isOpen ? 'chevron-up' : 'chevron-down'"
        :size="13"
        class="fluent-select-arrow"
      />
    </button>

    <div
      v-if="isOpen"
      :class="['fluent-select-menu', `placement-${placement}`]"
      @mousedown.stop
    >
      <button
        v-for="opt in options"
        :key="opt.value"
        type="button"
        :class="['fluent-select-option', { 'is-active': opt.value === modelValue }]"
        @click="selectOption(opt.value)"
      >
        <span class="opt-text">{{ opt.label }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import SvgIcon from './SvgIcon.vue';

export interface SelectOption {
  value: string;
  label: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: string;
    options: SelectOption[];
    placement?: 'top' | 'bottom';
    disabled?: boolean;
  }>(),
  {
    placement: 'bottom',
    disabled: false,
  }
);

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'change', value: string): void;
}>();

const rootRef = ref<HTMLElement | null>(null);
const isOpen = ref(false);

const currentLabel = computed(() => {
  const found = props.options.find((o) => o.value === props.modelValue);
  return found ? found.label : props.options[0]?.label || '';
});

function toggleOpen() {
  if (props.disabled) return;
  isOpen.value = !isOpen.value;
}

function selectOption(val: string) {
  isOpen.value = false;
  if (val !== props.modelValue) {
    emit('update:modelValue', val);
    emit('change', val);
  }
}

function handleOutsideMouseDown(e: MouseEvent) {
  if (!isOpen.value) return;
  const target = e.target as Node | null;
  if (rootRef.value && target && !rootRef.value.contains(target)) {
    isOpen.value = false;
  }
}

function handleKeyDown(e: KeyboardEvent) {
  if (isOpen.value && e.key === 'Escape') {
    isOpen.value = false;
  }
}

onMounted(() => {
  window.addEventListener('mousedown', handleOutsideMouseDown);
  window.addEventListener('keydown', handleKeyDown);
});

onUnmounted(() => {
  window.removeEventListener('mousedown', handleOutsideMouseDown);
  window.removeEventListener('keydown', handleKeyDown);
});
</script>

<style scoped>
.fluent-select-wrap {
  position: relative;
  display: inline-flex;
  min-width: 96px;
}

.fluent-select-trigger {
  width: 100%;
  height: 100%;
  min-height: 28px;
  padding: 4px 10px;
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-subtle);
  background: var(--bg-card);
  color: var(--text-primary);
  font-size: 12px;
  transition: background-color var(--duration-fast), border-color var(--duration-fast);
}

.fluent-select-trigger:hover:not(:disabled) {
  background: var(--bg-card-hover);
}

.fluent-select-wrap.is-open .fluent-select-trigger {
  border-color: var(--accent-color);
  background: var(--bg-card-hover);
}

.fluent-select-label {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: left;
}

.fluent-select-arrow {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.fluent-select-menu {
  position: absolute;
  left: 0;
  min-width: 100%;
  max-height: 240px;
  overflow-y: auto;
  padding: 5px;
  background: var(--bg-popover);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.18);
  backdrop-filter: blur(24px);
  -webkit-backdrop-filter: blur(24px);
  z-index: 360;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.fluent-select-menu.placement-bottom {
  top: calc(100% + 6px);
}

.fluent-select-menu.placement-top {
  bottom: calc(100% + 6px);
}

.fluent-select-option {
  position: relative;
  width: 100%;
  padding: 6px 12px;
  border-radius: var(--radius-sm);
  font-size: 12.5px;
  color: var(--text-primary);
  text-align: left;
  display: flex;
  align-items: center;
  white-space: nowrap;
  transition: background-color var(--duration-fast), color var(--duration-fast);
}

.fluent-select-option:hover {
  background: var(--bg-card-hover);
}

.fluent-select-option.is-active {
  background: var(--bg-card-hover);
  color: var(--accent-color);
  font-weight: 600;
}

.fluent-select-option.is-active::before {
  content: '';
  position: absolute;
  left: 3px;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 14px;
  border-radius: var(--radius-full);
  background: var(--accent-color);
}

.opt-text {
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
