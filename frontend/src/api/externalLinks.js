import api from './http';

export const fetchExternalLinks = (params = {}) => api.get('/external_links', { params });
