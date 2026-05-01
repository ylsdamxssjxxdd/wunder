import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerSharedHelpers } from './messengerControllerSharedHelpers';
import { installMessengerControllerCoreState } from './messengerControllerCoreState';
import { installMessengerControllerNavigationLists } from './messengerControllerNavigationLists';
import { installMessengerControllerConversationRuntime } from './messengerControllerConversationRuntime';
import { installMessengerControllerMessageResources } from './messengerControllerMessageResources';
import { installMessengerControllerWorkspaceActions } from './messengerControllerWorkspaceActions';
import { installMessengerControllerMessagingSettings } from './messengerControllerMessagingSettings';
import { installMessengerControllerLifecycle } from './messengerControllerLifecycle';

const installers = [
  installMessengerControllerSharedHelpers,
  installMessengerControllerCoreState,
  installMessengerControllerNavigationLists,
  installMessengerControllerConversationRuntime,
  installMessengerControllerMessageResources,
  installMessengerControllerWorkspaceActions,
  installMessengerControllerMessagingSettings,
  installMessengerControllerLifecycle
];

export function installMessengerController(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
