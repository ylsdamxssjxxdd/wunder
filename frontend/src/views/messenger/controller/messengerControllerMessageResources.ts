import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerMessagePanelsPresentation } from './messengerControllerMessagePanelsPresentation';
import { installMessengerControllerWorkspaceResourceHydration } from './messengerControllerWorkspaceResourceHydration';
import { installMessengerControllerMessageMarkdownVoice } from './messengerControllerMessageMarkdownVoice';
import { installMessengerControllerMessageRoutingPreferences } from './messengerControllerMessageRoutingPreferences';

const installers = [
  installMessengerControllerMessagePanelsPresentation,
  installMessengerControllerWorkspaceResourceHydration,
  installMessengerControllerMessageMarkdownVoice,
  installMessengerControllerMessageRoutingPreferences,
];

export function installMessengerControllerMessageResources(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
