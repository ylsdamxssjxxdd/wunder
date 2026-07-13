export const shouldAbortBeeroomDispatchStreamOnReset = (options: {
  keepSending?: boolean;
}) => options.keepSending !== true;
