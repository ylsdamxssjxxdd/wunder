import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerFileToolSettings } from './messengerControllerFileToolSettings';
import { installMessengerControllerAgentMessageCommands } from './messengerControllerAgentMessageCommands';
import { installMessengerControllerWorldMessagingActions } from './messengerControllerWorldMessagingActions';
import { installMessengerControllerClientPreferenceActions } from './messengerControllerClientPreferenceActions';

const installers = [
  installMessengerControllerFileToolSettings,
  installMessengerControllerAgentMessageCommands,
  installMessengerControllerWorldMessagingActions,
  installMessengerControllerClientPreferenceActions,
];

export function installMessengerControllerMessagingSettings(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
