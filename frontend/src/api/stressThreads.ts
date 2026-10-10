import api from './http';

export interface StressThreadStartPayload {
  user_rounds: number;
  model_rounds: number;
  title?: string;
}

export interface StressThreadJobSnapshot {
  job_id: string;
  session_id: string;
  total_rounds: number;
  created_time: number;
  status: {
    state: 'running' | 'completed' | 'failed';
    done_rounds?: number;
    items_written?: number;
    tool_calls?: number;
    session_id?: string;
    error?: string;
  };
}

export const startStressThread = (payload: StressThreadStartPayload) =>
  api.post('/chat/stress-threads', payload);

export const getStressThreadJob = (jobId: string) =>
  api.get(`/chat/stress-threads/${encodeURIComponent(jobId)}`);
