import api from './http';

export type SystemAnnouncement = {
  revision: string;
  title: string;
  content: string;
  updated_at: string;
};

export const fetchSystemAnnouncement = () => api.get('/announcement');
