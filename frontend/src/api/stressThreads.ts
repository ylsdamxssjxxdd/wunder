import api from './http';

export interface StressThreadStartPayload {
  user_rounds: number;
  model_rounds: number;
  title?: string;
  seed?: number;
  scenario?: 'typical' | 'complex' | 'adversarial';
  /** Input alias retained for older clients. */
  profile?: 'typical' | 'complex' | 'adversarial';
}

export interface StressThreadStorageBytes {
  rollout_raw_bytes: number;
  rollout_zstd_bytes: number;
  state_bytes: number;
  state_wal_bytes: number;
  queue_bytes: number;
  queue_wal_bytes: number;
  thread_history_bytes: number;
  thread_history_wal_bytes: number;
  total_bytes: number;
}

export interface StressThreadJobSnapshot {
  job_id: string;
  session_id: string;
  total_rounds: number;
  requested_user_rounds?: number;
  requested_model_rounds_per_turn?: number;
  requested_seed?: number;
  created_time: number;
  status: {
    state: 'running' | 'completed' | 'failed';
    done_rounds?: number;
    items_written?: number;
    items?: number;
    tool_calls?: number;
    model_requests?: number;
    model_actions?: number;
    rollout_records?: number;
    seed?: number;
    profile?: string;
    scenario?: 'typical' | 'complex' | 'adversarial';
    initial_context_tokens?: number;
    storage_bytes?: StressThreadStorageBytes;
    session_id?: string;
    error?: string;
  };
}

export const startStressThread = (payload: StressThreadStartPayload) =>
  api.post('/chat/stress-threads', payload);

export const getStressThreadJob = (jobId: string) =>
  api.get(`/chat/stress-threads/${encodeURIComponent(jobId)}`);

export const listStressThreads = () => api.get('/chat/stress-threads');
