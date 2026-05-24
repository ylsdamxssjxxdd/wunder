export type CompanionOpenHandler = (agentId: string) => Promise<void> | void;

let activeCompanionOpenHandler: CompanionOpenHandler | null = null;

export const registerCompanionOpenHandler = (handler: CompanionOpenHandler): (() => void) => {
  activeCompanionOpenHandler = handler;
  return () => {
    if (activeCompanionOpenHandler === handler) {
      activeCompanionOpenHandler = null;
    }
  };
};

export const openCompanionAgent = async (agentId: string): Promise<boolean> => {
  if (!activeCompanionOpenHandler) {
    return false;
  }
  await Promise.resolve(activeCompanionOpenHandler(agentId));
  return true;
};
