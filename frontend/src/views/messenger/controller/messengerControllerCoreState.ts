import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerStateRefs } from './messengerControllerStateRefs';
import { installMessengerControllerShellLayoutState } from './messengerControllerShellLayoutState';
import { installMessengerControllerAgentIdentityState } from './messengerControllerAgentIdentityState';

const installers = [
  installMessengerControllerStateRefs,
  installMessengerControllerShellLayoutState,
  installMessengerControllerAgentIdentityState,
];

export function installMessengerControllerCoreState(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
