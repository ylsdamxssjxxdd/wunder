import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerLifecycleRuntimeMeta } from './messengerControllerLifecycleRuntimeMeta';
import { installMessengerControllerLifecycleMessageViewport } from './messengerControllerLifecycleMessageViewport';
import { installMessengerControllerLifecycleRouteBootstrap } from './messengerControllerLifecycleRouteBootstrap';
import { installMessengerControllerLifecycleReactiveEffects } from './messengerControllerLifecycleReactiveEffects';

const installers = [
  installMessengerControllerLifecycleRuntimeMeta,
  installMessengerControllerLifecycleMessageViewport,
  installMessengerControllerLifecycleRouteBootstrap,
  installMessengerControllerLifecycleReactiveEffects,
];

export function installMessengerControllerLifecycle(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
