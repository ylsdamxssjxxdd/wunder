import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerWorkspaceOrderUiActions } from './messengerControllerWorkspaceOrderUiActions';
import { installMessengerControllerBeeroomAgentMutations } from './messengerControllerBeeroomAgentMutations';
import { installMessengerControllerConversationOpenActions } from './messengerControllerConversationOpenActions';
import { installMessengerControllerTimelineFileActions } from './messengerControllerTimelineFileActions';

const installers = [
  installMessengerControllerWorkspaceOrderUiActions,
  installMessengerControllerBeeroomAgentMutations,
  installMessengerControllerConversationOpenActions,
  installMessengerControllerTimelineFileActions,
];

export function installMessengerControllerWorkspaceActions(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
