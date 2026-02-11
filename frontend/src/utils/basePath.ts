import { isDesktopModeEnabled } from '@/config/desktop';

export const resolveUserBasePath = (path: string): '/demo' | '/desktop' | '/app' => {
  if (path.startsWith('/demo')) {
    return '/demo';
  }
  if (path.startsWith('/desktop')) {
    return '/desktop';
  }
  if (isDesktopModeEnabled()) {
    return '/desktop';
  }
  return '/app';
};

export const isDesktopPath = (path: string): boolean => resolveUserBasePath(path) === '/desktop';
