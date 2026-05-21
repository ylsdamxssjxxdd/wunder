import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerStateRefs } from './messengerControllerStateRefs';
import { installMessengerControllerShellLayoutState } from './messengerControllerShellLayoutState';
import { installMessengerControllerRenderableMessages } from './messengerControllerRenderableMessages';
import { installMessengerControllerAgentIdentityState } from './messengerControllerAgentIdentityState';

const installers = [
  installMessengerControllerStateRefs,
  installMessengerControllerShellLayoutState,
  installMessengerControllerRenderableMessages,
  installMessengerControllerAgentIdentityState,
];

export function installMessengerControllerCoreState(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
