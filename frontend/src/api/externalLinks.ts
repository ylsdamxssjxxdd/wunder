import api from './http';

import type { QueryParams } from './types';

export const fetchExternalLinks = (params: QueryParams = {}) => api.get('/external_links', { params });
