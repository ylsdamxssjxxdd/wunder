import type { MessengerControllerContext } from './messengerControllerContext';
import { installMessengerControllerRuntimeToolLists } from './messengerControllerRuntimeToolLists';
import { installMessengerControllerContactLists } from './messengerControllerContactLists';
import { installMessengerControllerHiveMixedLists } from './messengerControllerHiveMixedLists';
import { installMessengerControllerPanelSummaries } from './messengerControllerPanelSummaries';

const installers = [
  installMessengerControllerRuntimeToolLists,
  installMessengerControllerContactLists,
  installMessengerControllerHiveMixedLists,
  installMessengerControllerPanelSummaries,
];

export function installMessengerControllerNavigationLists(ctx: MessengerControllerContext): void {
  for (const install of installers) {
    install(ctx);
  }
}
