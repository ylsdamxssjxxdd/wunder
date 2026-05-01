import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerAgentUnreadRuntime } from './messengerControllerAgentUnreadRuntime';
import { installMessengerControllerWorldComposerRuntime } from './messengerControllerWorldComposerRuntime';
import { installMessengerControllerRightDockSessionRuntime } from './messengerControllerRightDockSessionRuntime';
import { installMessengerControllerAgentRuntimeSignals } from './messengerControllerAgentRuntimeSignals';
import { installMessengerControllerRenderableMessages } from './messengerControllerRenderableMessages';

const installers = [
  installMessengerControllerAgentUnreadRuntime,
  installMessengerControllerWorldComposerRuntime,
  installMessengerControllerRightDockSessionRuntime,
  installMessengerControllerAgentRuntimeSignals,
  installMessengerControllerRenderableMessages,
];

export function installMessengerControllerConversationRuntime(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
