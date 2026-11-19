<script setup lang="ts">
import { computed } from "vue";
import type { BridgeStatus, DeviceInfo } from "../types";
import { useI18n } from "vue-i18n";
import { connectionStatusPresentation } from "../utils/connectionStatus";

const { t } = useI18n();

const props = defineProps<{
  status: BridgeStatus;
  loading: boolean;
  device?: DeviceInfo;
  atvvOk?: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
}>();

const presentation = computed(() => connectionStatusPresentation(props.status, props.device ? { ...props.device, atvv_ok: props.atvvOk } : undefined));
const statusText = computed(() => t(presentation.value.labelKey));
const statusDetail = computed(() => presentation.value.detail);

function buttonText(status: BridgeStatus): string {
  if (props.device?.auto_connect_enabled) return t("status.disconnect");
  if (status === "Connected") return t("status.disconnect");
  if (status === "Connecting") return t("status.connecting");
  return t("status.connect");
}
</script>

<template>
  <div class="device-status page-heading-actions">
    <span
      :class="['status-indicator page-action is-muted', presentation.tone]"
      :title="statusDetail || undefined"
      :aria-label="statusDetail ? `${statusText}：${statusDetail}` : statusText"
    >
      <span class="page-action-symbol" aria-hidden="true"><span class="dot status-light" :class="presentation.tone"></span></span>
      {{ statusText }}
    </span>
    <button type="button"
      :class="['connection-action page-action', (device?.auto_connect_enabled || status === 'Connected') ? 'disconnect is-danger' : 'connect']"
      :disabled="loading || (status === 'Connecting' && device?.auto_connect_enabled === undefined)"
      @click="emit('toggle')"
    >
      {{ loading ? t("common.processing") : buttonText(status) }}
    </button>
  </div>
</template>

<style scoped>
.status-indicator { cursor: default; }
.dot { --status-light-color: var(--text-muted); }
.connected .dot { --status-light-color: var(--success); }
.connecting .dot { --status-light-color: var(--warning); }
.disconnected .dot, .error .dot { --status-light-color: var(--danger); }
.connected .page-action-symbol { background: var(--success-bg); }
.connecting .page-action-symbol { background: var(--warning-bg); }
.disconnected .page-action-symbol, .error .page-action-symbol { background: var(--danger-bg); }
.connection-action:disabled { opacity: .6; }
</style>
