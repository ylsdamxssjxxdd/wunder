<template>
  <section class="messenger-settings-card desktop-model-settings-panel">
    <section v-if="selectedModel" class="desktop-model-settings-detail">
      <div class="desktop-model-settings-head">
        <div class="desktop-model-settings-title">
          {{ selectedModel.key || t('desktop.system.modelUnnamed') }}
        </div>
        <div class="desktop-model-settings-actions">
          <el-button
            class="desktop-model-settings-btn"
            size="small"
            :loading="savingModel"
            @click="setCurrentAsDefault"
          >
            {{ setCurrentDefaultLabel }}
          </el-button>
          <el-button
            class="desktop-model-settings-btn desktop-model-settings-btn--primary"
            size="small"
            :loading="savingModel"
            @click="saveModelSettings"
          >
            {{ t('desktop.common.save') }}
          </el-button>
          <el-button
            class="desktop-model-settings-btn desktop-model-settings-btn--danger"
            size="small"
            @click="removeModel(selectedModel)"
          >
            {{ t('desktop.common.remove') }}
          </el-button>
        </div>
      </div>

      <div class="desktop-model-settings-group">
        <div class="desktop-model-settings-section-head">
          <div class="desktop-model-settings-section-title">
            <i class="fa-solid fa-gear" aria-hidden="true"></i>
            <span>{{ t('desktop.system.section.basic') }}</span>
          </div>
        </div>
        <div class="desktop-model-settings-grid">
          <label class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.modelKey') }}</span>
            <el-input v-model="selectedModel.key" />
          </label>
          <label class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.modelType') }}</span>
            <el-select
              v-model="selectedModel.model_type"
              class="desktop-model-settings-input"
              popper-class="desktop-model-settings-popper"
            >
              <el-option :label="t('desktop.system.modelTypeLlm')" value="llm" />
              <el-option :label="t('desktop.system.modelTypeEmbedding')" value="embedding" />
              <el-option :label="t('desktop.system.modelTypeAsr')" value="asr" />
              <el-option :label="t('desktop.system.modelTypeTts')" value="tts" />
              <el-option :label="t('desktop.system.modelTypeImage')" value="image" />
              <el-option :label="t('desktop.system.modelTypeVideo')" value="video" />
            </el-select>
          </label>
          <label class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.provider') }}</span>
            <el-select
              v-model="selectedModel.provider"
              class="desktop-model-settings-input"
              popper-class="desktop-model-settings-popper"
              @change="handleProviderChange"
            >
              <el-option
                v-for="provider in providerOptionsForSelectedModel"
                :key="provider.id"
                :label="provider.label"
                :value="provider.id"
              />
            </el-select>
          </label>
          <label class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.modelName') }}</span>
            <el-input
              v-if="selectedProviderUsesManualModelInput"
              v-model="selectedModel.model"
              class="desktop-model-settings-input"
              :placeholder="modelNamePlaceholder"
              @input="handleModelInput"
              @blur="handleModelBlur"
            />
            <el-autocomplete
              v-else
              v-model="selectedModel.model"
              class="desktop-model-settings-input"
              popper-class="desktop-model-settings-popper"
              :fetch-suggestions="queryModelSuggestions"
              :placeholder="modelNamePlaceholder"
              :trigger-on-focus="false"
              clearable
              @input="handleModelInput"
              @select="handleModelSuggestionSelect"
              @blur="handleModelBlur"
            />
          </label>
          <div v-if="selectedProviderIsVirtualReplay" class="desktop-model-settings-field-hint desktop-model-settings-field--full">
            {{ t('desktop.system.virtualReplayHint') }}
          </div>
          <div v-if="selectedProviderIsVirtualReplay" class="desktop-model-settings-virtual desktop-model-settings-field--full">
            <div class="desktop-model-settings-virtual-head">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.virtualReplayLogs') }}</span>
              <div class="desktop-model-settings-actions">
                <el-button
                  class="desktop-model-settings-btn"
                  size="small"
                  :loading="loadingVirtualLogs"
                  @click="loadVirtualReplayLogs(true)"
                >
                  {{ t('common.refresh') }}
                </el-button>
                <el-upload
                  :auto-upload="false"
                  :show-file-list="false"
                  accept=".jsonl,application/jsonlines,application/json"
                  :on-change="handleVirtualReplayFileChange"
                >
                  <el-button
                    class="desktop-model-settings-btn"
                    size="small"
                    :loading="uploadingVirtualLog"
                  >
                    {{ t('common.upload') }}
                  </el-button>
                </el-upload>
              </div>
            </div>
            <div class="desktop-model-settings-inline">
              <el-select
                v-model="selectedVirtualReplayLogId"
                class="desktop-model-settings-input"
                popper-class="desktop-model-settings-popper"
                :loading="loadingVirtualLogs"
                clearable
                :placeholder="t('desktop.system.virtualReplayEmpty')"
                @change="handleVirtualReplayLogSelection"
              >
                <el-option
                  :label="t('desktop.system.virtualReplayRandomOption')"
                  value=""
                />
                <el-option
                  v-for="log in virtualReplayLogs"
                  :key="log.id"
                  :label="formatVirtualReplayLogLabel(log)"
                  :value="log.id"
                />
              </el-select>
              <div class="desktop-model-settings-actions">
                <el-button
                  class="desktop-model-settings-btn"
                  size="small"
                  :disabled="!selectedVirtualReplayLog"
                  :loading="updatingVirtualLog"
                  @click="toggleSelectedVirtualReplayLog"
                >
                  {{
                    selectedVirtualReplayLog?.enabled
                      ? t('desktop.system.virtualReplayDisable')
                      : t('desktop.system.virtualReplayEnable')
                  }}
                </el-button>
                <el-button
                  class="desktop-model-settings-btn desktop-model-settings-btn--danger"
                  size="small"
                  :disabled="!selectedVirtualReplayLog"
                  :loading="deletingVirtualLog"
                  @click="deleteSelectedVirtualReplayLog"
                >
                  {{ t('common.delete') }}
                </el-button>
              </div>
            </div>
            <div class="desktop-model-settings-field-hint">
              {{ selectedVirtualReplayStatus }}
            </div>
          </div>
          <label v-if="!selectedProviderIsVirtualReplay" class="desktop-model-settings-field desktop-model-settings-field--full">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.baseUrl') }}</span>
            <el-input
              v-model="selectedModel.base_url"
              :placeholder="modelBaseUrlPlaceholder"
            />
          </label>
          <label v-if="!selectedProviderIsVirtualReplay" class="desktop-model-settings-field desktop-model-settings-field--full">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.apiKey') }}</span>
            <el-input v-model="selectedModel.api_key" show-password />
          </label>
        </div>
      </div>

      <div class="desktop-model-settings-group">
        <div class="desktop-model-settings-section-head">
          <div class="desktop-model-settings-section-title">
            <i class="fa-solid fa-sliders" aria-hidden="true"></i>
            <span>{{ modelParameterSectionTitle }}</span>
          </div>
        </div>
        <div v-if="selectedModel.model_type !== 'embedding'" class="desktop-model-settings-grid">
          <label v-if="selectedModel.model_type === 'llm'" class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.temperature') }}</span>
            <el-input v-model="selectedModel.temperature" />
          </label>
          <label v-if="selectedModel.model_type === 'llm'" class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.timeout') }}</span>
            <el-input v-model="selectedModel.timeout_s" />
          </label>
          <label v-if="selectedModel.model_type === 'llm'" class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.maxOutput') }}</span>
            <el-input v-model="selectedModel.max_output" />
          </label>
          <label v-if="selectedModel.model_type === 'llm'" class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.maxRounds') }}</span>
            <el-input v-model="selectedModel.max_rounds" />
          </label>
          <label v-if="selectedModel.model_type === 'llm'" class="desktop-model-settings-field desktop-model-settings-field--full">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.maxContext') }}</span>
            <div class="desktop-model-settings-inline">
              <el-input v-model="selectedModel.max_context" />
              <el-button
                class="desktop-model-settings-btn"
                :loading="probingContext"
                :disabled="selectedProviderIsVirtualReplay"
                @click="probeMaxContext"
              >
                {{ t('desktop.system.maxContextProbe') }}
              </el-button>
            </div>
          </label>
          <template v-if="selectedModel.model_type === 'asr'">
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.asrLanguage') }}</span>
              <el-input
                v-model="selectedModel.asr_language"
                :placeholder="t('desktop.system.asrLanguagePlaceholder')"
              />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.asrResponseFormat') }}</span>
              <el-select
                v-model="selectedModel.asr_response_format"
                class="desktop-model-settings-input"
                popper-class="desktop-model-settings-popper"
              >
                <el-option label="json" value="json" />
                <el-option label="text" value="text" />
                <el-option label="verbose_json" value="verbose_json" />
                <el-option label="srt" value="srt" />
                <el-option label="vtt" value="vtt" />
              </el-select>
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.asrTemperature') }}</span>
              <el-input v-model="selectedModel.asr_temperature" />
            </label>
            <label class="desktop-model-settings-field desktop-model-settings-field--full">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.asrPrompt') }}</span>
              <el-input v-model="selectedModel.asr_prompt" type="textarea" :rows="2" />
            </label>
          </template>
          <template v-if="selectedModel.model_type === 'tts'">
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.ttsVoice') }}</span>
              <el-select
                v-if="ttsVoiceOptions.length"
                v-model="selectedModel.tts_voice"
                class="desktop-model-settings-input"
                popper-class="desktop-model-settings-popper"
                filterable
                allow-create
                default-first-option
                clearable
                :loading="probingTtsVoices"
                :placeholder="t('desktop.system.ttsVoicePlaceholder')"
              >
                <el-option
                  v-for="voice in ttsVoiceOptions"
                  :key="voice"
                  :label="voice"
                  :value="voice"
                />
              </el-select>
              <el-input
                v-else
                v-model="selectedModel.tts_voice"
                :placeholder="t('desktop.system.ttsVoicePlaceholder')"
              />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.ttsResponseFormat') }}</span>
              <el-select
                v-model="selectedModel.tts_response_format"
                class="desktop-model-settings-input"
                popper-class="desktop-model-settings-popper"
              >
                <el-option label="wav" value="wav" />
                <el-option label="mp3" value="mp3" />
                <el-option label="flac" value="flac" />
                <el-option label="aac" value="aac" />
                <el-option label="opus" value="opus" />
                <el-option label="pcm" value="pcm" />
              </el-select>
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.ttsSpeed') }}</span>
              <el-input v-model="selectedModel.tts_speed" />
            </label>
            <label class="desktop-model-settings-field desktop-model-settings-field--full">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.ttsInstructions') }}</span>
              <el-input v-model="selectedModel.tts_instructions" type="textarea" :rows="2" />
            </label>
          </template>
          <template v-if="selectedModel.model_type === 'image'">
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.imageSize') }}</span>
              <el-input v-model="selectedModel.image_size" :placeholder="t('desktop.system.imageSizePlaceholder')" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.imageOutputFormat') }}</span>
              <el-select
                v-model="selectedModel.image_output_format"
                class="desktop-model-settings-input"
                popper-class="desktop-model-settings-popper"
              >
                <el-option :label="t('desktop.system.modelDefaultOption')" value="" />
                <el-option label="png" value="png" />
                <el-option label="jpeg" value="jpeg" />
                <el-option label="webp" value="webp" />
              </el-select>
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.imageSteps') }}</span>
              <el-input v-model="selectedModel.image_num_inference_steps" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.imageGuidanceScale') }}</span>
              <el-input v-model="selectedModel.image_guidance_scale" />
            </label>
            <label class="desktop-model-settings-field desktop-model-settings-field--full">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.imageNegativePrompt') }}</span>
              <el-input v-model="selectedModel.image_negative_prompt" type="textarea" :rows="2" />
            </label>
          </template>
          <template v-if="selectedModel.model_type === 'video'">
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoSize') }}</span>
              <el-input v-model="selectedModel.video_size" :placeholder="t('desktop.system.videoSizePlaceholder')" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoSeconds') }}</span>
              <el-input v-model="selectedModel.video_seconds" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoFps') }}</span>
              <el-input v-model="selectedModel.video_fps" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoNumFrames') }}</span>
              <el-input v-model="selectedModel.video_num_frames" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoSteps') }}</span>
              <el-input v-model="selectedModel.video_num_inference_steps" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoGuidanceScale') }}</span>
              <el-input v-model="selectedModel.video_guidance_scale" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoGuidanceScale2') }}</span>
              <el-input v-model="selectedModel.video_guidance_scale_2" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoBoundaryRatio') }}</span>
              <el-input v-model="selectedModel.video_boundary_ratio" />
            </label>
            <label class="desktop-model-settings-field">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoFlowShift') }}</span>
              <el-input v-model="selectedModel.video_flow_shift" />
            </label>
            <label class="desktop-model-settings-field desktop-model-settings-field--full">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoNegativePrompt') }}</span>
              <el-input v-model="selectedModel.video_negative_prompt" type="textarea" :rows="2" />
            </label>
            <div class="desktop-model-settings-field desktop-model-settings-field--full">
              <span class="desktop-model-settings-field-label">{{ t('desktop.system.videoAdvanced') }}</span>
              <div class="desktop-model-settings-checkbox-group">
                <label class="desktop-model-settings-checkbox">
                  <input v-model="selectedModel.video_enable_frame_interpolation" type="checkbox" />
                  <span>{{ t('desktop.system.videoEnableFrameInterpolation') }}</span>
                </label>
              </div>
            </div>
          </template>
        </div>
        <div v-else class="desktop-model-settings-section-empty">
          {{ t('desktop.system.sectionConnectionOnly') }}
        </div>
      </div>

      <div class="desktop-model-settings-group">
        <div class="desktop-model-settings-section-head">
          <div class="desktop-model-settings-section-title">
            <i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true"></i>
            <span>{{ t('desktop.system.section.capabilities') }}</span>
          </div>
        </div>
        <div v-if="selectedModel.model_type === 'llm' && !selectedProviderIsVirtualReplay" class="desktop-model-settings-grid">
          <div class="desktop-model-settings-field desktop-model-settings-field--full">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.capabilityToggle') }}</span>
            <div class="desktop-model-settings-checkbox-group">
              <label class="desktop-model-settings-checkbox">
                <input v-model="selectedModel.support_vision" type="checkbox" />
                <span>{{ t('desktop.system.supportVision') }}</span>
              </label>
              <label class="desktop-model-settings-checkbox">
                <input v-model="selectedModel.support_hearing" type="checkbox" />
                <span>{{ t('desktop.system.supportHearing') }}</span>
              </label>
              <label class="desktop-model-settings-checkbox">
                <input v-model="selectedModel.stream_include_usage" type="checkbox" />
                <span>{{ t('desktop.system.streamIncludeUsage') }}</span>
              </label>
            </div>
          </div>
          <label class="desktop-model-settings-field desktop-model-settings-field--full">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.toolCallMode') }}</span>
            <el-select
              v-model="selectedModel.tool_call_mode"
              class="desktop-model-settings-input"
              popper-class="desktop-model-settings-popper"
            >
              <el-option label="tool_call" value="tool_call" />
              <el-option label="function_call" value="function_call" />
              <el-option label="freeform_call" value="freeform_call" />
            </el-select>
          </label>
          <label class="desktop-model-settings-field desktop-model-settings-field--full">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.reasoningEffort') }}</span>
            <el-select
              v-model="selectedModel.reasoning_effort"
              class="desktop-model-settings-input"
              popper-class="desktop-model-settings-popper"
            >
              <el-option :label="t('desktop.system.reasoningEffort.default')" value="" />
              <el-option :label="t('desktop.system.reasoningEffort.none')" value="none" />
              <el-option :label="t('desktop.system.reasoningEffort.minimal')" value="minimal" />
              <el-option :label="t('desktop.system.reasoningEffort.low')" value="low" />
              <el-option :label="t('desktop.system.reasoningEffort.medium')" value="medium" />
              <el-option :label="t('desktop.system.reasoningEffort.high')" value="high" />
              <el-option :label="t('desktop.system.reasoningEffort.xhigh')" value="xhigh" />
            </el-select>
          </label>
        </div>
        <div v-else class="desktop-model-settings-section-empty">
          {{
            selectedProviderIsVirtualReplay
              ? t('desktop.system.virtualReplayCapabilities')
              : t('desktop.system.sectionLlmOnly')
          }}
        </div>
      </div>

      <div class="desktop-model-settings-group">
        <div class="desktop-model-settings-section-head">
          <div class="desktop-model-settings-section-title">
            <i class="fa-solid fa-compress" aria-hidden="true"></i>
            <span>{{ t('desktop.system.section.compaction') }}</span>
          </div>
        </div>
        <div v-if="selectedModel.model_type === 'llm' && !selectedProviderIsVirtualReplay" class="desktop-model-settings-grid">
          <label class="desktop-model-settings-field">
            <span class="desktop-model-settings-field-label">{{ t('desktop.system.historyCompactionRatio') }}</span>
            <el-input v-model="selectedModel.history_compaction_ratio" />
          </label>
        </div>
        <div v-else class="desktop-model-settings-section-empty">
          {{
            selectedProviderIsVirtualReplay
              ? t('desktop.system.virtualReplayCompaction')
              : t('desktop.system.sectionLlmOnly')
          }}
        </div>
      </div>
    </section>

    <section v-else class="desktop-model-settings-empty-panel">
      {{ t('desktop.system.modelDetailEmpty') }}
    </section>

    <HoneycombWaitingOverlay
      :visible="loading"
      :title="t('messenger.waiting.title')"
      :target-name="t('desktop.system.llm')"
      :phase-label="t('messenger.waiting.phase.preparing')"
      :summary-label="t('messenger.waiting.summary.desktopSettings')"
      :progress="34"
      :teleport-to-body="false"
    />
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteDesktopVirtualReplayLog,
  fetchDesktopSettings,
  listDesktopVirtualReplayLogs,
  probeDesktopLlmContextWindow,
  probeDesktopTtsVoices,
  setDesktopVirtualReplayLogEnabled,
  type DesktopVirtualReplayLog,
  uploadDesktopVirtualReplayLog,
  updateDesktopSettings
} from '@/api/desktop';
import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import { useI18n } from '@/i18n';
import {
  getProviderModelPresets,
  type ProviderModelType,
  resolveProviderModelPresetMaxContext
} from '@/views/messenger/providerModelPresets';

type ModelType = 'llm' | 'embedding' | 'asr' | 'tts' | 'image' | 'video';
type ToolCallMode = 'tool_call' | 'function_call' | 'freeform_call';
type ReasoningEffort = '' | 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh';
type SelectedModelPreference = {
  key: string;
  modelType: ModelType;
};
type ModelRow = {
  uid: string;
  key: string;
  model_type: ModelType;
  provider: string;
  base_url: string;
  api_key: string;
  model: string;
  temperature: string;
  timeout_s: string;
  max_rounds: string;
  max_output: string;
  max_context: string;
  support_vision: boolean;
  support_hearing: boolean;
  stream_include_usage: boolean;
  tool_call_mode: ToolCallMode;
  reasoning_effort: ReasoningEffort;
  history_compaction_ratio: string;
  asr_language: string;
  asr_prompt: string;
  asr_response_format: string;
  asr_temperature: string;
  tts_voice: string;
  tts_instructions: string;
  tts_response_format: string;
  tts_speed: string;
  image_size: string;
  image_output_format: string;
  image_negative_prompt: string;
  image_num_inference_steps: string;
  image_guidance_scale: string;
  video_size: string;
  video_seconds: string;
  video_fps: string;
  video_num_frames: string;
  video_negative_prompt: string;
  video_num_inference_steps: string;
  video_guidance_scale: string;
  video_guidance_scale_2: string;
  video_boundary_ratio: string;
  video_flow_shift: string;
  video_enable_frame_interpolation: boolean;
  raw: Record<string, unknown>;
};

const EMBEDDING_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_embedding_model';
const ASR_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_asr_model';
const TTS_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_tts_model';
const IMAGE_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_image_model';
const VIDEO_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_video_model';
const VIRTUAL_REPLAY_PROVIDER_ID = 'virtual_replay';

const props = withDefaults(
  defineProps<{
    selectedModelKey?: string;
    createModelRequest?: {
      nonce: number;
      modelType?: string;
    };
  }>(),
  {
    selectedModelKey: '',
    createModelRequest: () => ({
      nonce: 0,
      modelType: 'llm'
    })
  }
);

const emit = defineEmits<{
  (event: 'desktop-model-meta-changed'): void;
  (event: 'desktop-model-rows-change', value: Array<Record<string, unknown>>): void;
  (event: 'desktop-model-selection-change', value: string): void;
}>();

const { t } = useI18n();

const loading = ref(false);
const savingModel = ref(false);
const probingContext = ref(false);
const defaultModel = ref('');
const defaultEmbeddingModel = ref('');
const defaultAsrModel = ref('');
const defaultTtsModel = ref('');
const defaultImageModel = ref('');
const defaultVideoModel = ref('');
const modelRows = ref<ModelRow[]>([]);
const selectedModelUid = ref('');
const probingTtsVoices = ref(false);
const ttsVoiceOptions = ref<string[]>([]);
const lastTtsVoiceProbeKey = ref('');
const virtualReplayLogs = ref<DesktopVirtualReplayLog[]>([]);
const selectedVirtualReplayLogId = ref('');
const loadingVirtualLogs = ref(false);
const uploadingVirtualLog = ref(false);
const updatingVirtualLog = ref(false);
const deletingVirtualLog = ref(false);
const virtualLogsLoaded = ref(false);
let nextModelUid = 1;

const DEFAULT_PROVIDER_ID = 'openai_compatible';
const PROVIDER_PRESETS_BY_TYPE: Record<
  ModelType,
  Array<{ id: string; label: string; baseUrl: string }>
> = {
  llm: [
    { id: VIRTUAL_REPLAY_PROVIDER_ID, label: 'virtual_replay', baseUrl: '' },
    { id: 'openai_compatible', label: 'openai_compatible', baseUrl: '' },
    { id: 'openai', label: 'openai', baseUrl: 'https://api.openai.com/v1' },
    { id: 'anthropic', label: 'anthropic', baseUrl: 'https://api.anthropic.com/v1' },
    { id: 'openrouter', label: 'openrouter', baseUrl: 'https://openrouter.ai/api/v1' },
    { id: 'siliconflow', label: 'siliconflow', baseUrl: 'https://api.siliconflow.cn/v1' },
    { id: 'deepseek', label: 'deepseek', baseUrl: 'https://api.deepseek.com' },
    { id: 'moonshot', label: 'moonshot', baseUrl: 'https://api.moonshot.ai/v1' },
    { id: 'qwen', label: 'qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1' },
    { id: 'groq', label: 'groq', baseUrl: 'https://api.groq.com/openai/v1' },
    { id: 'mistral', label: 'mistral', baseUrl: 'https://api.mistral.ai/v1' },
    { id: 'together', label: 'together', baseUrl: 'https://api.together.xyz/v1' },
    { id: 'ollama', label: 'ollama', baseUrl: 'http://127.0.0.1:11434/v1' },
    { id: 'lmstudio', label: 'lmstudio', baseUrl: 'http://127.0.0.1:1234/v1' }
  ],
  embedding: [
    { id: 'vllm_omni', label: 'vllm_omni', baseUrl: 'http://127.0.0.1:8000/v1' },
    { id: 'openai_compatible', label: 'openai_compatible', baseUrl: '' },
    { id: 'siliconflow', label: 'siliconflow', baseUrl: 'https://api.siliconflow.cn/v1' },
    { id: 'qwen', label: 'qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1' },
    { id: 'ollama', label: 'ollama', baseUrl: 'http://127.0.0.1:11434/v1' },
    { id: 'lmstudio', label: 'lmstudio', baseUrl: 'http://127.0.0.1:1234/v1' }
  ],
  asr: [
    { id: 'vllm_omni', label: 'vllm-omni', baseUrl: 'http://127.0.0.1:8000/v1' },
    { id: 'whisper_cpp', label: 'whisper', baseUrl: 'http://127.0.0.1:8080' }
  ],
  tts: [
    { id: 'vllm_omni', label: 'vllm-omni', baseUrl: 'http://127.0.0.1:8000/v1' }
  ],
  image: [
    { id: 'vllm_omni', label: 'vllm-omni', baseUrl: 'http://127.0.0.1:8000/v1' }
  ],
  video: [
    { id: 'vllm_omni', label: 'vllm-omni', baseUrl: 'http://127.0.0.1:8000/v1' }
  ]
};
const PROVIDER_PRESET_MAP = new Map(
  Object.values(PROVIDER_PRESETS_BY_TYPE)
    .flat()
    .map((item) => [item.id, item])
);

const makeModelUid = (): string => `desktop-model-${nextModelUid++}`;
const selectedModel = computed(
  () => modelRows.value.find((item) => item.uid === selectedModelUid.value) || null
);

const isDefaultModelRow = (row: ModelRow): boolean => {
  const key = row.key.trim();
  if (!key) return false;
  switch (row.model_type) {
    case 'embedding':
      return key === defaultEmbeddingModel.value.trim();
    case 'asr':
      return key === defaultAsrModel.value.trim();
    case 'tts':
      return key === defaultTtsModel.value.trim();
    case 'image':
      return key === defaultImageModel.value.trim();
    case 'video':
      return key === defaultVideoModel.value.trim();
    default:
      return key === defaultModel.value.trim();
  }
};

const buildModelRowsSnapshot = (): Array<Record<string, unknown>> =>
  modelRows.value.map((row) => ({
    uid: row.uid,
    key: row.key,
    model_type: row.model_type,
    provider: row.provider,
    base_url: row.base_url,
    model: row.model,
    is_default: isDefaultModelRow(row)
  }));

const emitModelRowsChange = () => {
  emit('desktop-model-rows-change', buildModelRowsSnapshot());
};

const emitSelectedModelChange = (uid: string = selectedModelUid.value) => {
  emit('desktop-model-selection-change', String(uid || '').trim());
};

const normalizeModelType = (value: unknown): ModelType => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'embedding' || raw === 'embed' || raw === 'embeddings') {
    return 'embedding';
  }
  if (
    raw === 'asr' ||
    raw === 'stt' ||
    raw === 'speech_to_text' ||
    raw === 'audio_transcription' ||
    raw === 'transcription' ||
    raw === 'audio_to_text'
  ) {
    return 'asr';
  }
  if (raw === 'tts' || raw === 'speech' || raw === 'text_to_speech' || raw === 'text-to-speech') {
    return 'tts';
  }
  if (raw === 'image' || raw === 'draw' || raw === 'drawing' || raw === 'text_to_image' || raw === 'text-to-image') {
    return 'image';
  }
  if (
    raw === 'video' ||
    raw === 'movie' ||
    raw === 'animation' ||
    raw === 'text_to_video' ||
    raw === 'text-to-video'
  ) {
    return 'video';
  }
  return 'llm';
};

const normalizeProviderId = (value: unknown): string => {
  const raw = String(value || '').trim();
  if (!raw) {
    return DEFAULT_PROVIDER_ID;
  }
  const normalized = raw.toLowerCase().replace(/[\s-]+/g, '_');
  switch (normalized) {
    case 'openai_compat':
      return 'openai_compatible';
    case 'openai_native':
      return 'openai';
    case 'claude':
    case 'anthropic_api':
      return 'anthropic';
    case 'silicon_flow':
      return 'siliconflow';
    case 'kimi':
      return 'moonshot';
    case 'dashscope':
      return 'qwen';
    case 'vllmomni':
      return 'vllm_omni';
    case 'whispercpp':
      return 'whisper_cpp';
    case 'lm_studio':
      return 'lmstudio';
    case 'virtual':
    case 'virtual_llm':
    case 'virtual_model':
    case 'replay':
    case 'jsonl_replay':
    case 'mock_replay':
      return VIRTUAL_REPLAY_PROVIDER_ID;
    default:
      return normalized;
  }
};

const isVirtualReplayProvider = (provider: unknown): boolean =>
  normalizeProviderId(provider) === VIRTUAL_REPLAY_PROVIDER_ID;
const getDefaultProviderIdForType = (modelType: ModelType): string =>
  modelType === 'llm' ? DEFAULT_PROVIDER_ID : 'vllm_omni';
const getProviderPresetsForType = (
  modelType: ModelType
): Array<{ id: string; label: string; baseUrl: string }> =>
  PROVIDER_PRESETS_BY_TYPE[modelType] || PROVIDER_PRESETS_BY_TYPE.llm;
const getProviderPreset = (provider: unknown) => PROVIDER_PRESET_MAP.get(normalizeProviderId(provider));
const resolveProviderBaseUrl = (provider: unknown): string => getProviderPreset(provider)?.baseUrl || '';
const resolveDefaultToolCallMode = (provider: unknown): ToolCallMode =>
  normalizeProviderId(provider) === 'openai' ? 'freeform_call' : 'function_call';

const normalizeToolCallMode = (value: unknown, provider?: unknown): ToolCallMode => {
  const normalized = String(value || '').trim().toLowerCase();
  if (!normalized) {
    return resolveDefaultToolCallMode(provider);
  }
  if (normalized === 'freeform_call' || normalized === 'freeform') return 'freeform_call';
  if (normalized === 'function_call') return 'function_call';
  if (normalized === 'tool_call') return 'tool_call';
  return resolveDefaultToolCallMode(provider);
};

const normalizeReasoningEffort = (value: unknown): ReasoningEffort => {
  const raw = String(value || '').trim().toLowerCase().replace(/[\s-]+/g, '_');
  if (!raw || raw === 'default' || raw === 'auto' || raw === 'inherit') {
    return '';
  }
  if (raw === 'none' || raw === 'off' || raw === 'disable' || raw === 'disabled') {
    return 'none';
  }
  if (raw === 'minimal' || raw === 'min') return 'minimal';
  if (raw === 'low') return 'low';
  if (raw === 'medium' || raw === 'med' || raw === 'normal') return 'medium';
  if (raw === 'high') return 'high';
  if (raw === 'xhigh' || raw === 'x_high' || raw === 'extra_high' || raw === 'very_high') {
    return 'xhigh';
  }
  return '';
};

const normalizeTtsResponseFormat = (value: unknown): string => {
  const raw = String(value || '').trim().toLowerCase();
  return ['wav', 'mp3', 'flac', 'aac', 'opus', 'pcm'].includes(raw) ? raw : 'wav';
};

const normalizeAsrResponseFormat = (value: unknown): string => {
  const raw = String(value || '').trim().toLowerCase();
  return ['json', 'text', 'verbose_json', 'srt', 'vtt'].includes(raw) ? raw : 'json';
};

const normalizeImageOutputFormat = (value: unknown): string => {
  const raw = String(value || '').trim().toLowerCase();
  return ['png', 'jpeg', 'webp'].includes(raw) ? raw : '';
};

const FLOAT_INPUT_PRECISION = 7;
const roundFloat = (value: number): number => {
  const factor = 10 ** FLOAT_INPUT_PRECISION;
  return Math.round(value * factor) / factor;
};
const trimTrailingZeros = (valueText: string): string => {
  if (!valueText.includes('.')) {
    return valueText;
  }
  const trimmed = valueText.replace(/(?:\.0+|(\.\d*?[1-9])0+)$/, '$1').replace(/\.$/, '');
  return trimmed === '-0' ? '0' : trimmed;
};
const formatFloatForInput = (value: unknown, fallback: number): string => {
  const numeric = typeof value === 'number' ? value : Number.parseFloat(String(value ?? ''));
  const resolved = Number.isFinite(numeric) ? numeric : fallback;
  if (!Number.isFinite(resolved)) {
    return '';
  }
  return trimTrailingZeros(roundFloat(resolved).toFixed(FLOAT_INPUT_PRECISION));
};

const applyModelPresetContext = (row: ModelRow, force = true) => {
  if (!row || normalizeModelType(row.model_type) !== 'llm') return;
  if (!force && String(row.max_context || '').trim()) return;
  const maxContext = resolveProviderModelPresetMaxContext(row.provider, row.model, 'llm');
  if (!Number.isFinite(maxContext) || Number(maxContext) <= 0) return;
  row.max_context = String(Math.round(Number(maxContext)));
};

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    uid: makeModelUid(),
    key,
    model_type: normalizeModelType(raw.model_type),
    provider: normalizeProviderId(
      raw.provider || getDefaultProviderIdForType(normalizeModelType(raw.model_type))
    ),
    base_url: String(raw.base_url || ''),
    api_key: String(raw.api_key || ''),
    model: String(raw.model || ''),
    temperature: formatFloatForInput(raw.temperature, 0.7),
    timeout_s: raw.timeout_s == null ? '120' : String(raw.timeout_s),
    max_rounds: raw.max_rounds == null ? '1000' : String(raw.max_rounds),
    max_output: raw.max_output == null ? '' : String(raw.max_output),
    max_context: raw.max_context == null ? '' : String(raw.max_context),
    support_vision: raw.support_vision === true,
    support_hearing: raw.support_hearing === true,
    stream_include_usage: raw.stream_include_usage !== false,
    tool_call_mode: normalizeToolCallMode(raw.tool_call_mode, raw.provider),
    reasoning_effort: normalizeReasoningEffort(raw.reasoning_effort),
    history_compaction_ratio: formatFloatForInput(raw.history_compaction_ratio, 0.9),
    asr_language: String(raw.asr_language || ''),
    asr_prompt: String(raw.asr_prompt || ''),
    asr_response_format: normalizeAsrResponseFormat(raw.asr_response_format),
    asr_temperature: formatFloatForInput(raw.asr_temperature, 0),
    tts_voice: String(raw.tts_voice || ''),
    tts_instructions: String(raw.tts_instructions || ''),
    tts_response_format: normalizeTtsResponseFormat(raw.tts_response_format),
    tts_speed: formatFloatForInput(raw.tts_speed, 1),
    image_size: String(raw.image_size || ''),
    image_output_format: normalizeImageOutputFormat(raw.image_output_format),
    image_negative_prompt: String(raw.image_negative_prompt || ''),
    image_num_inference_steps:
      raw.image_num_inference_steps == null ? '' : String(raw.image_num_inference_steps),
    image_guidance_scale: raw.image_guidance_scale == null ? '' : String(raw.image_guidance_scale),
    video_size: String(raw.video_size || ''),
    video_seconds: raw.video_seconds == null ? '' : String(raw.video_seconds),
    video_fps: raw.video_fps == null ? '' : String(raw.video_fps),
    video_num_frames: raw.video_num_frames == null ? '' : String(raw.video_num_frames),
    video_negative_prompt: String(raw.video_negative_prompt || ''),
    video_num_inference_steps:
      raw.video_num_inference_steps == null ? '' : String(raw.video_num_inference_steps),
    video_guidance_scale: raw.video_guidance_scale == null ? '' : String(raw.video_guidance_scale),
    video_guidance_scale_2:
      raw.video_guidance_scale_2 == null ? '' : String(raw.video_guidance_scale_2),
    video_boundary_ratio:
      raw.video_boundary_ratio == null ? '' : String(raw.video_boundary_ratio),
    video_flow_shift: raw.video_flow_shift == null ? '' : String(raw.video_flow_shift),
    video_enable_frame_interpolation: raw.video_enable_frame_interpolation === true,
    raw: { ...raw }
  }));

const reuseModelRowUids = (rows: ModelRow[], previousRows: ModelRow[]) => {
  const usedPreviousUids = new Set<string>();
  for (const row of rows) {
    const key = row.key.trim();
    if (!key) continue;
    const modelType = normalizeModelType(row.model_type);
    const matched =
      previousRows.find(
        (item) =>
          !usedPreviousUids.has(item.uid) &&
          item.key.trim() === key &&
          normalizeModelType(item.model_type) === modelType
      ) ||
      previousRows.find((item) => !usedPreviousUids.has(item.uid) && item.key.trim() === key);
    if (!matched) continue;
    usedPreviousUids.add(matched.uid);
    row.uid = matched.uid;
  }
};

const buildSelectedModelPreference = (row: ModelRow | null): SelectedModelPreference | null => {
  if (!row) return null;
  const key = row.key.trim();
  if (!key) return null;
  return {
    key,
    modelType: normalizeModelType(row.model_type)
  };
};

const resolveSelectedModelUid = (
  rows: ModelRow[],
  preference: SelectedModelPreference | null | undefined
): string => {
  const preferredKey = String(preference?.key || '').trim();
  if (!preferredKey) return '';
  const preferredType = normalizeModelType(preference?.modelType);
  return (
    rows.find(
      (item) => item.key.trim() === preferredKey && normalizeModelType(item.model_type) === preferredType
    )?.uid || rows.find((item) => item.key.trim() === preferredKey)?.uid || ''
  );
};

const ensureSelectedModel = (preference?: SelectedModelPreference | null) => {
  const previousUid = selectedModelUid.value;
  if (!modelRows.value.length) {
    selectedModelUid.value = '';
    if (previousUid) {
      emitSelectedModelChange('');
    }
    return;
  }
  const matchedUid = resolveSelectedModelUid(modelRows.value, preference);
  if (matchedUid) {
    selectedModelUid.value = matchedUid;
    if (previousUid !== selectedModelUid.value) {
      emitSelectedModelChange(selectedModelUid.value);
    }
    return;
  }
  if (!modelRows.value.some((item) => item.uid === selectedModelUid.value)) {
    selectedModelUid.value = modelRows.value[0].uid;
  }
  if (previousUid !== selectedModelUid.value) {
    emitSelectedModelChange(selectedModelUid.value);
  }
};

const findDefaultModelKeyByType = (
  rows: ModelRow[],
  modelType: ModelType,
  desiredKey: string
): string => {
  const desired = String(desiredKey || '').trim();
  if (desired) {
    const matched = rows.find(
      (item) => item.key.trim() === desired && normalizeModelType(item.model_type) === modelType
    );
    if (matched) {
      return matched.key.trim();
    }
  }
  return rows.find((item) => normalizeModelType(item.model_type) === modelType)?.key.trim() || '';
};

const defaultModelStorageKeyByType = (modelType: ModelType): string => {
  switch (modelType) {
    case 'embedding':
      return EMBEDDING_DEFAULT_MODEL_STORAGE_KEY;
    case 'asr':
      return ASR_DEFAULT_MODEL_STORAGE_KEY;
    case 'tts':
      return TTS_DEFAULT_MODEL_STORAGE_KEY;
    case 'image':
      return IMAGE_DEFAULT_MODEL_STORAGE_KEY;
    case 'video':
      return VIDEO_DEFAULT_MODEL_STORAGE_KEY;
    default:
      return '';
  }
};

const readStoredDefaultModel = (modelType: ModelType): string => {
  const storageKey = defaultModelStorageKeyByType(modelType);
  if (!storageKey) return '';
  try {
    return String(localStorage.getItem(storageKey) || '').trim();
  } catch {
    return '';
  }
};

const writeStoredDefaultModel = (modelType: ModelType, modelName: string): void => {
  const storageKey = defaultModelStorageKeyByType(modelType);
  if (!storageKey) return;
  const normalized = String(modelName || '').trim();
  try {
    if (normalized) {
      localStorage.setItem(storageKey, normalized);
    } else {
      localStorage.removeItem(storageKey);
    }
  } catch {
    // ignore localStorage failures
  }
};

const providerOptionsForSelectedModel = computed(() => {
  const modelType = normalizeModelType(selectedModel.value?.model_type);
  const currentProvider = normalizeProviderId(selectedModel.value?.provider);
  const options = getProviderPresetsForType(modelType).map((item) => ({
    id: item.id,
    label: item.label
  }));
  if (currentProvider && !PROVIDER_PRESET_MAP.has(currentProvider)) {
    options.unshift({ id: currentProvider, label: currentProvider });
  }
  return options;
});

const selectedProviderIsVirtualReplay = computed(() => isVirtualReplayProvider(selectedModel.value?.provider));

const selectedProviderUsesManualModelInput = computed(() => {
  const current = selectedModel.value;
  if (!current) return true;
  const modelType = normalizeModelType(current.model_type);
  if (modelType !== 'llm') {
    return true;
  }
  return normalizeProviderId(current.provider) === 'openai_compatible' || isVirtualReplayProvider(current.provider);
});

const modelOptionsForSelectedModel = computed(() => {
  const current = selectedModel.value;
  if (!current) return [];
  if (normalizeProviderId(current.provider) === 'openai_compatible') return [];

  const options: Array<{ value: string; label: string }> = [];
  const existing = new Set<string>();
  for (const preset of getProviderModelPresets(
    current.provider,
    normalizeModelType(current.model_type) as ProviderModelType
  )) {
    const modelId = String(preset.id || '').trim();
    if (!modelId) continue;
    const normalized = modelId.toLowerCase();
    if (existing.has(normalized)) continue;
    existing.add(normalized);
    options.push({
      value: modelId,
      label: preset.label || modelId
    });
  }

  const currentModelId = String(current.model || '').trim();
  if (currentModelId) {
    const normalizedCurrent = currentModelId.toLowerCase();
    if (!existing.has(normalizedCurrent)) {
      options.unshift({
        value: currentModelId,
        label: currentModelId
      });
    }
  }

  return options;
});

const modelBaseUrlPlaceholder = computed(() => {
  const provider = selectedModel.value?.provider;
  return resolveProviderBaseUrl(provider) || t('desktop.system.baseUrlPlaceholder');
});

const modelNamePlaceholder = computed(() =>
  selectedProviderIsVirtualReplay.value
    ? t('desktop.system.virtualReplayModelPlaceholder')
    : t('desktop.system.modelNamePlaceholder')
);

const selectedVirtualReplayLog = computed(() => {
  const id = String(selectedVirtualReplayLogId.value || '').trim();
  if (!id) return null;
  return virtualReplayLogs.value.find((log) => log.id === id) || null;
});

const selectedVirtualReplayStatus = computed(() => {
  if (!virtualReplayLogs.value.length) {
    return t('desktop.system.virtualReplayEmptyHint');
  }
  const selected = selectedVirtualReplayLog.value;
  if (!selected) {
    return t('desktop.system.virtualReplayRandomStatus');
  }
  return t('desktop.system.virtualReplayStatus', {
    id: selected.id,
    rounds: selected.user_rounds || 0,
    state: selected.enabled
      ? t('desktop.system.virtualReplayEnabled')
      : t('desktop.system.virtualReplayDisabled')
  });
});

const normalizeVirtualReplayLogs = (items: unknown): DesktopVirtualReplayLog[] =>
  (Array.isArray(items) ? items : [])
    .map((item) => {
      const raw = (item || {}) as Record<string, unknown>;
      return {
        id: String(raw.id || '').trim(),
        name: String(raw.name || raw.id || '').trim(),
        enabled: raw.enabled !== false,
        format: String(raw.format || '').trim(),
        user_rounds: Number.isFinite(Number(raw.user_rounds)) ? Number(raw.user_rounds) : 0,
        size_bytes: Number.isFinite(Number(raw.size_bytes)) ? Number(raw.size_bytes) : 0,
        uploaded_at: String(raw.uploaded_at || '').trim()
      };
    })
    .filter((log) => log.id);

const resolveRequestErrorMessage = (error: unknown, fallbackKey: string): string => {
  const responseDetail = (error as { response?: { data?: { detail?: unknown; message?: unknown } } })?.response
    ?.data;
  return (
    String(responseDetail?.detail || responseDetail?.message || (error as { message?: unknown })?.message || '').trim() ||
    t(fallbackKey)
  );
};

const syncVirtualReplaySelectionFromModel = () => {
  if (!selectedProviderIsVirtualReplay.value) {
    selectedVirtualReplayLogId.value = '';
    return;
  }
  const model = String(selectedModel.value?.model || '').trim();
  selectedVirtualReplayLogId.value = virtualReplayLogs.value.some((log) => log.id === model) ? model : '';
};

const loadVirtualReplayLogs = async (force = false) => {
  if (loadingVirtualLogs.value) return;
  if (virtualLogsLoaded.value && !force) {
    syncVirtualReplaySelectionFromModel();
    return;
  }
  loadingVirtualLogs.value = true;
  try {
    const response = await listDesktopVirtualReplayLogs();
    virtualReplayLogs.value = normalizeVirtualReplayLogs(response?.data?.logs);
    virtualLogsLoaded.value = true;
    syncVirtualReplaySelectionFromModel();
  } catch (error) {
    console.error(error);
    ElMessage.error(resolveRequestErrorMessage(error, 'desktop.system.virtualReplayLoadFailed'));
  } finally {
    loadingVirtualLogs.value = false;
  }
};

const formatVirtualReplayLogLabel = (log: DesktopVirtualReplayLog): string => {
  const name = log.name || log.id;
  const state = log.enabled
    ? t('desktop.system.virtualReplayEnabled')
    : t('desktop.system.virtualReplayDisabled');
  return `${name} - ${t('desktop.system.virtualReplayRounds', { rounds: log.user_rounds || 0 })} - ${state}`;
};

const handleVirtualReplayLogSelection = (value: string | number) => {
  const current = selectedModel.value;
  if (!current || !selectedProviderIsVirtualReplay.value) return;
  const id = String(value || '').trim();
  selectedVirtualReplayLogId.value = id;
  current.model = id;
};

const handleVirtualReplayFileChange = async (uploadFile: { raw?: File; name?: string }) => {
  const file = uploadFile?.raw;
  if (!file) {
    ElMessage.warning(t('desktop.system.virtualReplayFileRequired'));
    return;
  }
  uploadingVirtualLog.value = true;
  try {
    const response = await uploadDesktopVirtualReplayLog(file, uploadFile.name || file.name);
    virtualReplayLogs.value = normalizeVirtualReplayLogs(response?.data?.logs);
    virtualLogsLoaded.value = true;
    const uploadedId = String(response?.data?.log?.id || '').trim();
    if (uploadedId) {
      handleVirtualReplayLogSelection(uploadedId);
    } else {
      syncVirtualReplaySelectionFromModel();
    }
    ElMessage.success(t('desktop.system.virtualReplayUploadSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(resolveRequestErrorMessage(error, 'desktop.system.virtualReplayUploadFailed'));
  } finally {
    uploadingVirtualLog.value = false;
  }
};

const toggleSelectedVirtualReplayLog = async () => {
  const selected = selectedVirtualReplayLog.value;
  if (!selected || updatingVirtualLog.value) return;
  updatingVirtualLog.value = true;
  try {
    const response = await setDesktopVirtualReplayLogEnabled(selected.id, !selected.enabled);
    virtualReplayLogs.value = normalizeVirtualReplayLogs(response?.data?.logs);
    virtualLogsLoaded.value = true;
    syncVirtualReplaySelectionFromModel();
    ElMessage.success(t('desktop.system.virtualReplayUpdateSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(resolveRequestErrorMessage(error, 'desktop.system.virtualReplayUpdateFailed'));
  } finally {
    updatingVirtualLog.value = false;
  }
};

const deleteSelectedVirtualReplayLog = async () => {
  const selected = selectedVirtualReplayLog.value;
  if (!selected || deletingVirtualLog.value) return;
  try {
    await ElMessageBox.confirm(
      t('desktop.system.virtualReplayDeleteConfirm', { name: selected.name || selected.id }),
      t('common.delete'),
      {
        type: 'warning',
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
        confirmButtonClass: 'el-button--danger'
      }
    );
  } catch {
    return;
  }
  deletingVirtualLog.value = true;
  try {
    const response = await deleteDesktopVirtualReplayLog(selected.id);
    virtualReplayLogs.value = normalizeVirtualReplayLogs(response?.data?.logs);
    virtualLogsLoaded.value = true;
    if (selectedModel.value?.model === selected.id) {
      handleVirtualReplayLogSelection('');
    } else {
      syncVirtualReplaySelectionFromModel();
    }
    ElMessage.success(t('desktop.system.virtualReplayDeleteSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(resolveRequestErrorMessage(error, 'desktop.system.virtualReplayDeleteFailed'));
  } finally {
    deletingVirtualLog.value = false;
  }
};

const setCurrentDefaultLabel = computed(() => {
  const current = selectedModel.value;
  if (!current) return t('desktop.system.setDefaultChatModel');
  switch (normalizeModelType(current.model_type)) {
    case 'embedding':
      return t('desktop.system.setDefaultEmbeddingModel');
    case 'asr':
      return t('desktop.system.setDefaultAsrModel');
    case 'tts':
      return t('desktop.system.setDefaultTtsModel');
    case 'image':
      return t('desktop.system.setDefaultImageModel');
    case 'video':
      return t('desktop.system.setDefaultVideoModel');
    default:
      return t('desktop.system.setDefaultChatModel');
  }
});

const modelParameterSectionTitle = computed(() => {
  const current = selectedModel.value;
  switch (normalizeModelType(current?.model_type)) {
    case 'asr':
      return t('desktop.system.section.asr');
    case 'tts':
      return t('desktop.system.section.tts');
    case 'image':
      return t('desktop.system.section.image');
    case 'video':
      return t('desktop.system.section.video');
    default:
      return t('desktop.system.section.generation');
  }
});

const handleProviderChange = (value: string) => {
  const current = selectedModel.value;
  if (!current) return;
  const previousProvider = current.provider;
  const nextProvider = normalizeProviderId(value);
  current.provider = nextProvider;
  if (isVirtualReplayProvider(nextProvider)) {
    current.base_url = '';
    current.api_key = '';
    syncVirtualReplaySelectionFromModel();
    void loadVirtualReplayLogs();
  }
  const prevDefault = resolveDefaultToolCallMode(previousProvider);
  const currentMode = normalizeToolCallMode(current.tool_call_mode, previousProvider);
  if (!current.tool_call_mode || currentMode === prevDefault) {
    current.tool_call_mode = resolveDefaultToolCallMode(nextProvider);
  }
  applyModelPresetContext(current);
};

const queryModelSuggestions = (
  queryString: string,
  callback: (items: Array<{ value: string }>) => void
) => {
  const keyword = String(queryString || '').trim().toLowerCase();
  const items = modelOptionsForSelectedModel.value
    .filter((option) => {
      if (!keyword) return true;
      return option.value.toLowerCase().includes(keyword) || option.label.toLowerCase().includes(keyword);
    })
    .map((option) => ({ value: option.value }));
  callback(items);
};

const handleModelInput = (value: string | number) => {
  const current = selectedModel.value;
  if (!current) return;
  current.model = String(value || '');
  applyModelPresetContext(current);
};

const handleModelBlur = (event: FocusEvent) => {
  const current = selectedModel.value;
  if (!current) return;
  const target = event.target as HTMLInputElement | null;
  current.model = String(target?.value ?? current.model ?? '').trim();
  applyModelPresetContext(current);
};

const handleModelSuggestionSelect = (item: { value?: string }) => {
  const current = selectedModel.value;
  if (!current) return;
  current.model = String(item?.value || '').trim();
  applyModelPresetContext(current);
};

const buildTtsVoiceProbePayload = (row: ModelRow | null) => {
  if (!row || normalizeModelType(row.model_type) !== 'tts') return null;
  const model = String(row.model || '').trim();
  const provider = normalizeProviderId(row.provider);
  const baseUrl = String(row.base_url || '').trim() || resolveProviderBaseUrl(provider);
  const apiKey = String(row.api_key || '').trim();
  if (!baseUrl || !model) return null;
  return {
    provider,
    base_url: baseUrl,
    api_key: apiKey || undefined,
    model,
    timeout_s: 15
  };
};

const probeTtsVoicesForSelectedModel = async (force = false) => {
  const current = selectedModel.value;
  const payload = buildTtsVoiceProbePayload(current);
  if (!payload) {
    ttsVoiceOptions.value = [];
    lastTtsVoiceProbeKey.value = '';
    return;
  }
  const probeKey = `${current?.uid || ''}|${payload.provider}|${payload.base_url}|${payload.model}|${
    payload.api_key ? 1 : 0
  }`;
  if (!force && probeKey === lastTtsVoiceProbeKey.value) {
    return;
  }
  probingTtsVoices.value = true;
  try {
    const response = await probeDesktopTtsVoices(payload);
    const latest = selectedModel.value;
    if (!latest || latest.uid !== current?.uid) return;
    const items = Array.isArray(response?.data?.voices)
      ? response.data.voices
          .map((item: unknown) => String(item || '').trim())
          .filter((item: string) => item.length > 0)
      : [];
    ttsVoiceOptions.value = Array.from(new Set(items));
    lastTtsVoiceProbeKey.value = probeKey;
  } catch (error) {
    console.error(error);
    ttsVoiceOptions.value = [];
    lastTtsVoiceProbeKey.value = '';
  } finally {
    probingTtsVoices.value = false;
  }
};

const addModel = (modelType: ModelType = 'llm') => {
  const row: ModelRow = {
    uid: makeModelUid(),
    key: '',
    model_type: modelType,
    provider: getDefaultProviderIdForType(modelType),
    base_url: '',
    api_key: '',
    model: '',
    temperature: modelType === 'llm' ? '0.7' : '',
    timeout_s: '120',
    max_rounds: modelType === 'llm' ? '1000' : '',
    max_output: '',
    max_context: '',
    support_vision: false,
    support_hearing: false,
    stream_include_usage: true,
    tool_call_mode: resolveDefaultToolCallMode(getDefaultProviderIdForType(modelType)),
    reasoning_effort: '',
    history_compaction_ratio: modelType === 'llm' ? '0.9' : '',
    asr_language: '',
    asr_prompt: '',
    asr_response_format: 'json',
    asr_temperature: '0',
    tts_voice: '',
    tts_instructions: '',
    tts_response_format: 'wav',
    tts_speed: '1',
    image_size: '',
    image_output_format: '',
    image_negative_prompt: '',
    image_num_inference_steps: '',
    image_guidance_scale: '',
    video_size: '',
    video_seconds: '',
    video_fps: '',
    video_num_frames: '',
    video_negative_prompt: '',
    video_num_inference_steps: '',
    video_guidance_scale: '',
    video_guidance_scale_2: '',
    video_boundary_ratio: '',
    video_flow_shift: '',
    video_enable_frame_interpolation: false,
    raw: {}
  };
  modelRows.value.push(row);
  selectedModelUid.value = row.uid;
  emitSelectedModelChange(row.uid);
  emitModelRowsChange();
};

const setCurrentAsDefault = async () => {
  const current = selectedModel.value;
  if (!current) return;
  if (savingModel.value) return;
  const key = current.key.trim();
  if (!key) {
    ElMessage.warning(t('desktop.system.modelKeyRequired'));
    return;
  }
  const previousDefaultModel = defaultModel.value;
  const previousDefaultEmbeddingModel = defaultEmbeddingModel.value;
  const previousDefaultAsrModel = defaultAsrModel.value;
  const previousDefaultTtsModel = defaultTtsModel.value;
  const previousDefaultImageModel = defaultImageModel.value;
  const previousDefaultVideoModel = defaultVideoModel.value;
  const modelType = normalizeModelType(current.model_type);
  switch (modelType) {
    case 'embedding':
      defaultEmbeddingModel.value = key;
      break;
    case 'asr':
      defaultAsrModel.value = key;
      break;
    case 'tts':
      defaultTtsModel.value = key;
      break;
    case 'image':
      defaultImageModel.value = key;
      break;
    case 'video':
      defaultVideoModel.value = key;
      break;
    default:
      defaultModel.value = key;
      break;
  }
  const saved = await saveModelSettings();
  if (!saved) {
    defaultModel.value = previousDefaultModel;
    defaultEmbeddingModel.value = previousDefaultEmbeddingModel;
    defaultAsrModel.value = previousDefaultAsrModel;
    defaultTtsModel.value = previousDefaultTtsModel;
    defaultImageModel.value = previousDefaultImageModel;
    defaultVideoModel.value = previousDefaultVideoModel;
  }
};

const buildModelPayload = (row: ModelRow): Record<string, unknown> => {
  const output: Record<string, unknown> = {};

  const setText = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (cleaned) {
      output[key] = cleaned;
    } else {
      delete output[key];
    }
  };

  const setFloat = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (!cleaned) {
      delete output[key];
      return;
    }
    const parsed = Number.parseFloat(cleaned);
    if (Number.isFinite(parsed)) {
      output[key] = roundFloat(parsed);
    } else {
      delete output[key];
    }
  };

  const setInt = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (!cleaned) {
      delete output[key];
      return;
    }
    const parsed = Number.parseInt(cleaned, 10);
    if (Number.isFinite(parsed)) {
      output[key] = parsed;
    } else {
      delete output[key];
    }
  };

  setText('model_type', row.model_type);
  const provider = normalizeProviderId(row.provider);
  setText('provider', provider);
  if (!isVirtualReplayProvider(provider)) {
    setText('base_url', row.base_url);
    setText('api_key', row.api_key);
  }
  setText('model', row.model);

  if (row.model_type === 'llm') {
    setInt('timeout_s', row.timeout_s);
    setFloat('temperature', row.temperature);
    setInt('max_rounds', row.max_rounds);
    setInt('max_output', row.max_output);
    setInt('max_context', row.max_context);
    output.support_vision = row.support_vision === true;
    output.support_hearing = row.support_hearing === true;
    output.stream_include_usage = row.stream_include_usage !== false;
    setText('tool_call_mode', row.tool_call_mode);
    setText('reasoning_effort', normalizeReasoningEffort(row.reasoning_effort));
    setFloat('history_compaction_ratio', row.history_compaction_ratio);
  } else if (row.model_type === 'tts') {
    setText('tts_voice', row.tts_voice);
    setText('tts_instructions', row.tts_instructions);
    setText('tts_response_format', normalizeTtsResponseFormat(row.tts_response_format));
    setFloat('tts_speed', row.tts_speed);
  } else if (row.model_type === 'asr') {
    setText('asr_language', row.asr_language);
    setText('asr_prompt', row.asr_prompt);
    setText('asr_response_format', normalizeAsrResponseFormat(row.asr_response_format));
    setFloat('asr_temperature', row.asr_temperature);
  } else if (row.model_type === 'image') {
    setText('image_size', row.image_size);
    setText('image_output_format', normalizeImageOutputFormat(row.image_output_format));
    setText('image_negative_prompt', row.image_negative_prompt);
    setInt('image_num_inference_steps', row.image_num_inference_steps);
    setFloat('image_guidance_scale', row.image_guidance_scale);
  } else if (row.model_type === 'video') {
    setText('video_size', row.video_size);
    setFloat('video_seconds', row.video_seconds);
    setInt('video_fps', row.video_fps);
    setInt('video_num_frames', row.video_num_frames);
    setText('video_negative_prompt', row.video_negative_prompt);
    setInt('video_num_inference_steps', row.video_num_inference_steps);
    setFloat('video_guidance_scale', row.video_guidance_scale);
    setFloat('video_guidance_scale_2', row.video_guidance_scale_2);
    setFloat('video_boundary_ratio', row.video_boundary_ratio);
    setFloat('video_flow_shift', row.video_flow_shift);
    output.video_enable_frame_interpolation = row.video_enable_frame_interpolation === true;
  }

  return output;
};

const isBlankDraftModelRow = (row: ModelRow): boolean => {
  const textValues = [
    row.key,
    row.base_url,
    row.api_key,
    row.model,
    row.max_output,
    row.max_context,
    row.asr_language,
    row.asr_prompt,
    row.tts_voice,
    row.tts_instructions,
    row.image_size,
    row.image_negative_prompt,
    row.video_size,
    row.video_negative_prompt
  ];
  return textValues.every((value) => !String(value || '').trim());
};

const probeMaxContext = async () => {
  const current = selectedModel.value;
  if (!current || normalizeModelType(current.model_type) !== 'llm') return;
  const modelName = current.model.trim();
  if (!modelName) {
    ElMessage.warning(t('desktop.system.modelNameRequired'));
    return;
  }
  const targetUid = current.uid;
  probingContext.value = true;
  try {
    const response = await probeDesktopLlmContextWindow({
      provider: normalizeProviderId(current.provider),
      base_url: String(current.base_url || '').trim(),
      api_key: String(current.api_key || '').trim(),
      model: modelName,
      timeout_s: 15
    });
    const payload = (response?.data || {}) as Record<string, unknown>;
    const maxContext = Number(payload.max_context);
    const message = String(payload.message || '').trim();
    const latest = selectedModel.value;
    if (!latest || latest.uid !== targetUid) return;
    if (Number.isFinite(maxContext) && maxContext > 0) {
      latest.max_context = String(Math.round(maxContext));
      ElMessage.success(t('desktop.system.maxContextProbeSuccess', { value: latest.max_context }));
      return;
    }
    if (message) {
      ElMessage.info(message);
      return;
    }
    ElMessage.info(t('desktop.system.maxContextProbeNoResult'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.system.maxContextProbeFailed'));
  } finally {
    probingContext.value = false;
  }
};

const applySettingsData = (
  data: Record<string, any>,
  preferredSelection: SelectedModelPreference | null = buildSelectedModelPreference(selectedModel.value)
) => {
  const llm = data.llm || {};
  const previousRows = modelRows.value;
  const nextRows = parseModelRows((llm.models as Record<string, Record<string, unknown>>) || {});
  reuseModelRowUids(nextRows, previousRows);
  modelRows.value = nextRows;
  modelRows.value.forEach((row) => applyModelPresetContext(row, false));

  defaultModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'llm',
    String(llm.default || '').trim()
  );
  defaultEmbeddingModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'embedding',
    String(llm.default_embedding || '').trim() || readStoredDefaultModel('embedding')
  );
  defaultAsrModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'asr',
    String(llm.default_asr || '').trim() || readStoredDefaultModel('asr')
  );
  defaultTtsModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'tts',
    String(llm.default_tts || '').trim() || readStoredDefaultModel('tts')
  );
  defaultImageModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'image',
    String(llm.default_image || '').trim() || readStoredDefaultModel('image')
  );
  defaultVideoModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'video',
    String(llm.default_video || '').trim() || readStoredDefaultModel('video')
  );

  ensureSelectedModel(preferredSelection);
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const preferredSelection = buildSelectedModelPreference(selectedModel.value);
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data, preferredSelection);
    emitModelRowsChange();
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveModelSettings = async (options: { allowEmpty?: boolean } = {}): Promise<boolean> => {
  const models: Record<string, Record<string, unknown>> = {};
  const preferredSelection = buildSelectedModelPreference(selectedModel.value);
  const rowsForSave = modelRows.value.filter((row) => !isBlankDraftModelRow(row));
  const allowEmpty = options.allowEmpty === true;

  for (const row of rowsForSave) {
    const key = row.key.trim();
    if (!key) {
      ElMessage.warning(t('desktop.system.modelKeyRequired'));
      return false;
    }
    if (models[key]) {
      ElMessage.warning(t('desktop.system.modelKeyDuplicate', { key }));
      return false;
    }
    models[key] = buildModelPayload(row);
  }

  const hasEmbeddingRows = rowsForSave.some((row) => normalizeModelType(row.model_type) === 'embedding');
  const hasLlmRows = rowsForSave.some((row) => normalizeModelType(row.model_type) === 'llm');
  const currentDefaultModel = findDefaultModelKeyByType(
    rowsForSave,
    'llm',
    defaultModel.value.trim() || Object.keys(models)[0] || ''
  );
  if (!currentDefaultModel) {
    if (!allowEmpty || hasLlmRows) {
      ElMessage.warning(t('desktop.system.defaultModelRequired'));
      return false;
    }
  } else if (!models[currentDefaultModel]) {
    ElMessage.warning(t('desktop.system.defaultModelMissing'));
    return false;
  }

  const defaultModelConfig = models[currentDefaultModel] || {};
  const defaultProvider = String(defaultModelConfig.provider || '').trim();
  const defaultBaseUrl = String(defaultModelConfig.base_url || '').trim();
  const defaultModelName = String(defaultModelConfig.model || '').trim();
  if (
    currentDefaultModel &&
    !isVirtualReplayProvider(defaultProvider) &&
    (!defaultBaseUrl || !defaultModelName)
  ) {
    ElMessage.warning(t('desktop.system.defaultModelConfigRequired'));
    return false;
  }

  const currentDefaultEmbedding = findDefaultModelKeyByType(
    rowsForSave,
    'embedding',
    defaultEmbeddingModel.value.trim()
  );
  if (hasEmbeddingRows && !currentDefaultEmbedding) {
    ElMessage.warning(t('desktop.system.defaultEmbeddingModelRequired'));
    return false;
  }
  if (currentDefaultEmbedding && !models[currentDefaultEmbedding]) {
    ElMessage.warning(t('desktop.system.defaultEmbeddingModelMissing'));
    return false;
  }

  const currentDefaultAsr = findDefaultModelKeyByType(rowsForSave, 'asr', defaultAsrModel.value.trim());
  if (currentDefaultAsr && !models[currentDefaultAsr]) {
    ElMessage.warning(t('desktop.system.defaultAsrModelMissing'));
    return false;
  }

  const currentDefaultTts = findDefaultModelKeyByType(rowsForSave, 'tts', defaultTtsModel.value.trim());
  if (currentDefaultTts && !models[currentDefaultTts]) {
    ElMessage.warning(t('desktop.system.defaultTtsModelMissing'));
    return false;
  }

  const currentDefaultImage = findDefaultModelKeyByType(rowsForSave, 'image', defaultImageModel.value.trim());
  if (currentDefaultImage && !models[currentDefaultImage]) {
    ElMessage.warning(t('desktop.system.defaultImageModelMissing'));
    return false;
  }

  const currentDefaultVideo = findDefaultModelKeyByType(rowsForSave, 'video', defaultVideoModel.value.trim());
  if (currentDefaultVideo && !models[currentDefaultVideo]) {
    ElMessage.warning(t('desktop.system.defaultVideoModelMissing'));
    return false;
  }

  savingModel.value = true;
  try {
    const response = await updateDesktopSettings({
      llm: {
        default: currentDefaultModel,
        default_embedding: currentDefaultEmbedding || undefined,
        default_asr: currentDefaultAsr || undefined,
        default_tts: currentDefaultTts || undefined,
        default_image: currentDefaultImage || undefined,
        default_video: currentDefaultVideo || undefined,
        models
      }
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    writeStoredDefaultModel('embedding', currentDefaultEmbedding);
    writeStoredDefaultModel('asr', currentDefaultAsr);
    writeStoredDefaultModel('tts', currentDefaultTts);
    writeStoredDefaultModel('image', currentDefaultImage);
    writeStoredDefaultModel('video', currentDefaultVideo);
    applySettingsData(data, preferredSelection);
    emitModelRowsChange();
    emit('desktop-model-meta-changed');
    ElMessage.success(t('desktop.common.saveSuccess'));
    return true;
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
    return false;
  } finally {
    savingModel.value = false;
  }
};

const removeModel = async (target: ModelRow) => {
  if (savingModel.value) return;
  const targetName = target.key.trim() || target.model.trim() || t('desktop.system.modelUnnamed');
  try {
    await ElMessageBox.confirm(
      t('desktop.system.modelRemoveConfirm', { name: targetName }),
      t('common.delete'),
      {
        type: 'warning',
        confirmButtonText: t('desktop.common.remove'),
        cancelButtonText: t('common.cancel'),
        confirmButtonClass: 'el-button--danger'
      }
    );
  } catch {
    return;
  }

  const previousRows = modelRows.value.slice();
  const previousSelection = selectedModelUid.value;
  const previousDefaults = {
    defaultModel: defaultModel.value,
    defaultEmbeddingModel: defaultEmbeddingModel.value,
    defaultAsrModel: defaultAsrModel.value,
    defaultTtsModel: defaultTtsModel.value,
    defaultImageModel: defaultImageModel.value,
    defaultVideoModel: defaultVideoModel.value
  };
  modelRows.value = modelRows.value.filter((item) => item.uid !== target.uid);
  defaultModel.value = findDefaultModelKeyByType(modelRows.value, 'llm', defaultModel.value);
  defaultEmbeddingModel.value = findDefaultModelKeyByType(modelRows.value, 'embedding', defaultEmbeddingModel.value);
  defaultAsrModel.value = findDefaultModelKeyByType(modelRows.value, 'asr', defaultAsrModel.value);
  defaultTtsModel.value = findDefaultModelKeyByType(modelRows.value, 'tts', defaultTtsModel.value);
  defaultImageModel.value = findDefaultModelKeyByType(modelRows.value, 'image', defaultImageModel.value);
  defaultVideoModel.value = findDefaultModelKeyByType(modelRows.value, 'video', defaultVideoModel.value);
  ensureSelectedModel();
  emitModelRowsChange();
  const saved = await saveModelSettings({ allowEmpty: true });
  if (!saved) {
    modelRows.value = previousRows;
    selectedModelUid.value = previousSelection;
    defaultModel.value = previousDefaults.defaultModel;
    defaultEmbeddingModel.value = previousDefaults.defaultEmbeddingModel;
    defaultAsrModel.value = previousDefaults.defaultAsrModel;
    defaultTtsModel.value = previousDefaults.defaultTtsModel;
    defaultImageModel.value = previousDefaults.defaultImageModel;
    defaultVideoModel.value = previousDefaults.defaultVideoModel;
    emitSelectedModelChange(previousSelection);
    emitModelRowsChange();
  }
};

watch(
  () => [props.selectedModelKey, modelRows.value.length] as const,
  ([selectedModelKey]) => {
    const normalized = String(selectedModelKey || '').trim();
    if (!normalized) {
      return;
    }
    const matched = modelRows.value.find(
      (item) => String(item.uid || '').trim() === normalized || String(item.key || '').trim() === normalized
    );
    if (matched && selectedModelUid.value !== matched.uid) {
      selectedModelUid.value = matched.uid;
      if (normalized !== matched.uid) {
        emitSelectedModelChange(matched.uid);
      }
    }
  },
  { immediate: true }
);

watch(
  () => [props.createModelRequest?.nonce, props.createModelRequest?.modelType] as const,
  ([nonce, modelType], previous) => {
    if (!nonce || nonce === previous?.[0]) return;
    addModel(normalizeModelType(modelType || 'llm'));
  }
);

watch(
  () => {
    const current = selectedModel.value;
    return current ? `${current.uid}|${current.model_type}` : '';
  },
  (currentKey, previousKey) => {
    const row = selectedModel.value;
    if (!row || !currentKey || !previousKey) return;
    const [previousUid, previousTypeRaw] = previousKey.split('|');
    const [currentUid, currentTypeRaw] = currentKey.split('|');
    if (!previousUid || previousUid !== currentUid) return;
    const previousType = normalizeModelType(previousTypeRaw);
    const currentType = normalizeModelType(currentTypeRaw);
    if (previousType === currentType) return;
    const currentProvider = normalizeProviderId(row.provider);
    const desiredProvider = getDefaultProviderIdForType(currentType);
    const previousDefaultProvider = getDefaultProviderIdForType(previousType);
    const availableProviders = new Set(
      getProviderPresetsForType(currentType).map((item) => normalizeProviderId(item.id))
    );
    if (!availableProviders.has(currentProvider) || currentProvider === previousDefaultProvider) {
      const previousBaseUrl = String(row.base_url || '').trim();
      const currentBaseUrl = resolveProviderBaseUrl(currentProvider);
      row.provider = desiredProvider;
      if (!previousBaseUrl || previousBaseUrl === currentBaseUrl) {
        row.base_url = resolveProviderBaseUrl(desiredProvider);
      }
      row.tool_call_mode = resolveDefaultToolCallMode(desiredProvider);
    }
  }
);

watch(
  () => {
    const current = selectedModel.value;
    return current
      ? [
          current.uid,
          current.model_type,
          current.provider,
          current.base_url,
          current.api_key,
          current.model
        ].join('|')
      : '';
  },
  () => {
    void probeTtsVoicesForSelectedModel();
  },
  { immediate: true }
);

watch(
  () => {
    const current = selectedModel.value;
    return current ? `${current.uid}|${current.provider}|${current.model}` : '';
  },
  () => {
    if (!selectedProviderIsVirtualReplay.value) {
      selectedVirtualReplayLogId.value = '';
      return;
    }
    void loadVirtualReplayLogs();
    syncVirtualReplaySelectionFromModel();
  },
  { immediate: true }
);

watch(
  () => buildModelRowsSnapshot(),
  () => {
    emitModelRowsChange();
  },
  { deep: true }
);

onMounted(() => {
  void loadSettings();
});
</script>

<style scoped>
.desktop-model-settings-panel {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.desktop-model-settings-detail {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 0;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}

.desktop-model-settings-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
  padding-bottom: 12px;
  border-bottom: 1px solid #e4e9f0;
}

.desktop-model-settings-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--portal-text);
}

.desktop-model-settings-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.desktop-model-settings-group {
  display: grid;
  gap: 10px;
  padding-top: 12px;
  margin-top: 12px;
  border-top: 1px solid #e4e9f0;
}

.desktop-model-settings-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.desktop-model-settings-section-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-text);
}

.desktop-model-settings-section-title i {
  color: var(--portal-muted);
}

.desktop-model-settings-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.desktop-model-settings-field {
  display: grid;
  gap: 6px;
  min-width: 0;
  color: var(--portal-text);
}

.desktop-model-settings-field--full {
  grid-column: 1 / -1;
}

.desktop-model-settings-field-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-muted);
}

.desktop-model-settings-field-hint {
  min-width: 0;
  color: var(--portal-muted);
  font-size: 12px;
  line-height: 1.6;
}

.desktop-model-settings-input {
  width: 100%;
}

.desktop-model-settings-virtual {
  display: grid;
  gap: 10px;
  min-width: 0;
  padding: 12px;
  border: 1px solid #dfe5ee;
  border-radius: 10px;
  background: #f8fafc;
}

.desktop-model-settings-virtual-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-width: 0;
  flex-wrap: wrap;
}

.desktop-model-settings-inline {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
  align-items: center;
}

.desktop-model-settings-checkbox-group {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.desktop-model-settings-checkbox {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: 28px;
  padding: 5px 9px;
  border: 1px solid #d8dee8;
  border-radius: 9px;
  background: #f8fafc;
  color: var(--portal-text);
  font-size: 12px;
}

.desktop-model-settings-checkbox input {
  margin: 0;
}

.desktop-model-settings-btn {
  border-radius: 9px;
  border: 1px solid #d8dce4;
  background: #ffffff;
  color: #4b5563;
  font-weight: 600;
}

.desktop-model-settings-btn--primary {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  background: var(--hula-accent);
  color: #ffffff;
}

.desktop-model-settings-btn--danger {
  border-color: rgba(214, 77, 77, 0.35);
  background: #fff5f5;
  color: #b42318;
}

.desktop-model-settings-section-empty,
.desktop-model-settings-empty-panel {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 96px;
  border: 1px dashed #d8dee8;
  border-radius: 12px;
  background: #f8fafc;
  color: var(--portal-muted);
  font-size: 13px;
  text-align: center;
  padding: 18px;
}

.desktop-model-settings-empty-panel {
  flex: 1 1 auto;
}

:global(.desktop-model-settings-popper .el-select-dropdown__item:hover),
:global(.desktop-model-settings-popper .el-select-dropdown__item.is-selected) {
  background: rgba(var(--ui-accent-rgb), 0.12);
  color: var(--hula-accent);
}

@media (max-width: 900px) {
  .desktop-model-settings-grid {
    grid-template-columns: 1fr;
  }

  .desktop-model-settings-inline {
    grid-template-columns: 1fr;
  }

  .desktop-model-settings-virtual-head {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
