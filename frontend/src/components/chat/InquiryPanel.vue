<template>
  <div class="inquiry-panel" :class="{ 'is-expanded': showAll }">
    <div class="inquiry-head">
      <div v-if="!showAll" class="inquiry-question">{{ currentQuestion.question }}</div>
      <div v-else class="inquiry-question">{{ t('chat.inquiry.allQuestions') }}</div>
      <div class="inquiry-tools">
        <template v-if="paging && !showAll">
          <button
            type="button"
            class="inquiry-tool-btn inquiry-tool-btn--prev"
            :disabled="page === 0"
            :title="t('chat.inquiry.prevQuestion')"
            :aria-label="t('chat.inquiry.prevQuestion')"
            @click="stepPage(-1)"
          >
            <i class="fa-solid fa-chevron-left" aria-hidden="true"></i>
          </button>
          <span class="inquiry-page">{{ page + 1 }}/{{ questions.length }}</span>
          <button
            type="button"
            class="inquiry-tool-btn inquiry-tool-btn--next"
            :disabled="page >= questions.length - 1"
            :title="t('chat.inquiry.nextQuestion')"
            :aria-label="t('chat.inquiry.nextQuestion')"
            @click="stepPage(1)"
          >
            <i class="fa-solid fa-chevron-right" aria-hidden="true"></i>
          </button>
        </template>
        <button
          v-if="questions.length > 1"
          type="button"
          class="inquiry-tool-btn inquiry-tool-btn--expand"
          :title="showAll ? t('chat.inquiry.collapse') : t('chat.inquiry.expand')"
          :aria-label="showAll ? t('chat.inquiry.collapse') : t('chat.inquiry.expand')"
          @click="showAll = !showAll"
        >
          <i
            :class="
              showAll
                ? 'fa-solid fa-down-left-and-up-right-to-center'
                : 'fa-solid fa-up-right-and-down-left-from-center'
            "
            aria-hidden="true"
          ></i>
        </button>
      </div>
    </div>

    <div
      v-for="{ question, index: questionIndex } in visibleQuestions"
      :key="`q-${questionIndex}-${question.question}`"
      class="inquiry-group"
    >
      <div v-if="showAll && questions.length > 1" class="inquiry-group-title">
        {{ t('chat.inquiry.questionProgress', { index: questionIndex + 1, total: questions.length }) }}
        · {{ question.question }}
      </div>

      <button
        v-for="(option, optionIndex) in question.options"
        :key="`o-${questionIndex}-${optionIndex}`"
        type="button"
        :class="[
          'inquiry-option',
          { 'is-active': isOptionActive(questionIndex, optionIndex) }
        ]"
        @click="toggleOption(questionIndex, optionIndex)"
      >
        <span class="inquiry-mark" :class="{ 'is-radio': !question.multiple }" aria-hidden="true">
          <svg v-if="isOptionActive(questionIndex, optionIndex)" viewBox="0 0 12 12">
            <path d="M2.5 6.4 4.7 8.6 9.5 3.6" />
          </svg>
        </span>
        <span class="inquiry-option-body">
          <span class="inquiry-option-label">{{ option.label }}</span>
          <span v-if="option.recommended" class="inquiry-option-tag">
            {{ t('chat.inquiry.recommended') }}
          </span>
          <span v-if="option.description" class="inquiry-option-desc">{{ option.description }}</span>
        </span>
      </button>

      <div :class="['inquiry-other', { 'is-active': Boolean(answer(questionIndex).other) }]">
        <button
          type="button"
          class="inquiry-mark-btn"
          :aria-label="t('chat.inquiry.otherPlaceholder')"
          @click="focusOther(questionIndex)"
        >
          <span class="inquiry-mark" :class="{ 'is-radio': !question.multiple }" aria-hidden="true">
            <svg v-if="Boolean(answer(questionIndex).other)" viewBox="0 0 12 12">
              <path d="M2.5 6.4 4.7 8.6 9.5 3.6" />
            </svg>
          </span>
        </button>
        <input
          :ref="(el) => setOtherRef(questionIndex, el)"
          :value="answer(questionIndex).other"
          class="inquiry-other-input"
          type="text"
          :placeholder="t('chat.inquiry.otherPlaceholder')"
          @input="onOtherInput(questionIndex, $event)"
        />
        <div class="inquiry-pref">
          <button
            type="button"
            class="inquiry-pref-main"
            :class="{ 'is-active': answer(questionIndex).noPreference }"
            @click="markNoPreference(questionIndex)"
          >
            {{ t('chat.inquiry.noPreference') }}
          </button>
          <button
            type="button"
            class="inquiry-pref-caret"
            :title="t('chat.inquiry.quickAnswer')"
            :aria-label="t('chat.inquiry.quickAnswer')"
            :aria-expanded="menuQuestion === questionIndex"
            @click="toggleMenu(questionIndex)"
          >
            <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
          </button>
          <div v-if="menuQuestion === questionIndex" class="inquiry-pref-menu">
            <button type="button" @click="markNoPreference(questionIndex)">
              {{ t('chat.inquiry.noPreference') }}
            </button>
            <button
              v-if="hasRecommended(questionIndex)"
              type="button"
              @click="useRecommended(questionIndex)"
            >
              {{ t('chat.inquiry.useRecommended') }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

type InquiryOption = { label: string; description?: string; recommended?: boolean };
type InquiryQuestion = { question: string; options: InquiryOption[]; multiple: boolean };
type InquiryPanelData = { questions?: InquiryQuestion[] };
type InquiryAnswerDraft = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};
type AnswerState = { labels: number[]; other: string; noPreference: boolean };

const props = defineProps<{ panel: InquiryPanelData | null }>();

const emit = defineEmits<{ (event: 'update:selected', answers: InquiryAnswerDraft[]): void }>();

const { t } = useI18n();

const questions = computed<InquiryQuestion[]>(() =>
  (Array.isArray(props.panel?.questions) ? props.panel!.questions : [])
    .map((question) => ({
      question: String(question?.question ?? ''),
      multiple: question?.multiple === true,
      options: (Array.isArray(question?.options) ? question.options : [])
        .map((option) => ({
          label: String(option?.label ?? ''),
          description: String(option?.description ?? ''),
          recommended: option?.recommended === true
        }))
        .filter((option) => Boolean(option.label))
    }))
    .filter((question) => question.options.length > 0)
);

const page = ref(0);
const showAll = ref(false);
const menuQuestion = ref(-1);
const states = ref<Record<number, AnswerState>>({});
const otherRefs = new Map<number, HTMLInputElement>();

const paging = computed(() => questions.value.length > 1);
const currentQuestion = computed<InquiryQuestion>(
  () => questions.value[page.value] || { question: '', options: [], multiple: false }
);
const visibleQuestions = computed<Array<{ question: InquiryQuestion; index: number }>>(() => {
  const list = questions.value;
  if (showAll.value || !paging.value) {
    return list.map((question, index) => ({ question, index }));
  }
  // 分页模式必须带上真实题序：作答态是按题序存的，用循环下标会把第一题的选择串到当前页。
  const index = Math.min(page.value, list.length - 1);
  return [{ question: list[index], index }];
});
const questionSignature = computed(() =>
  questions.value.map((question) => question.question).join('\u0000')
);

const answer = (questionIndex: number): AnswerState =>
  states.value[questionIndex] || { labels: [], other: '', noPreference: false };

const patchAnswer = (questionIndex: number, patch: Partial<AnswerState>) => {
  states.value = { ...states.value, [questionIndex]: { ...answer(questionIndex), ...patch } };
};

const isOptionActive = (questionIndex: number, optionIndex: number) =>
  answer(questionIndex).labels.includes(optionIndex);

const hasRecommended = (questionIndex: number) =>
  questions.value[questionIndex]?.options.some((option) => option.recommended) === true;

const setOtherRef = (questionIndex: number, el: unknown) => {
  if (el) {
    otherRefs.set(questionIndex, el as HTMLInputElement);
  } else {
    otherRefs.delete(questionIndex);
  }
};

const emitDraft = () => {
  const payload: InquiryAnswerDraft[] = questions.value
    .map((question, questionIndex) => {
      const draft = answer(questionIndex);
      return {
        questionIndex,
        labels: draft.labels
          .map((index) => String(question.options[index]?.label ?? ''))
          .filter(Boolean),
        other: draft.other.trim(),
        noPreference: draft.noPreference
      };
    })
    .filter((item) => item.labels.length > 0 || Boolean(item.other) || item.noPreference);
  emit('update:selected', payload);
};

const toggleOption = (questionIndex: number, optionIndex: number) => {
  const draft = answer(questionIndex);
  const multiple = questions.value[questionIndex]?.multiple === true;
  let labels: number[];
  if (multiple) {
    labels = draft.labels.includes(optionIndex)
      ? draft.labels.filter((index) => index !== optionIndex)
      : [...draft.labels, optionIndex];
  } else if (draft.labels.length === 1 && draft.labels[0] === optionIndex) {
    labels = [];
  } else {
    labels = [optionIndex];
  }
  patchAnswer(questionIndex, { labels, noPreference: false });
  menuQuestion.value = -1;
  emitDraft();
};

const onOtherInput = (questionIndex: number, event: Event) => {
  const other = (event.target as HTMLInputElement).value;
  patchAnswer(questionIndex, { other, noPreference: other.trim() ? false : answer(questionIndex).noPreference });
  emitDraft();
};

const focusOther = (questionIndex: number) => {
  otherRefs.get(questionIndex)?.focus();
};

const markNoPreference = (questionIndex: number) => {
  patchAnswer(questionIndex, { labels: [], other: '', noPreference: true });
  menuQuestion.value = -1;
  emitDraft();
};

const useRecommended = (questionIndex: number) => {
  const options = questions.value[questionIndex]?.options || [];
  const indexes = options
    .map((option, index) => (option.recommended ? index : -1))
    .filter((index) => index >= 0);
  const labels = questions.value[questionIndex]?.multiple ? indexes : indexes.slice(0, 1);
  patchAnswer(questionIndex, { labels, other: '', noPreference: false });
  menuQuestion.value = -1;
  emitDraft();
};

const toggleMenu = (questionIndex: number) => {
  menuQuestion.value = menuQuestion.value === questionIndex ? -1 : questionIndex;
};

const stepPage = (delta: number) => {
  const next = page.value + delta;
  page.value = Math.min(Math.max(next, 0), Math.max(questions.value.length - 1, 0));
};

const reset = () => {
  states.value = {};
  otherRefs.clear();
  page.value = 0;
  showAll.value = false;
  menuQuestion.value = -1;
  emitDraft();
};

// 只按问题正文判重：同一批问题在流式更新里会换对象引用，不该清空用户已选的作答。
watch(questionSignature, reset);
</script>
