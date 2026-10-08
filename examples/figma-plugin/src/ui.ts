import { handleVontaqFSFigmaUiMessage } from '@vontaq/fs/figma';

declare const parent: { postMessage(message: unknown, targetOrigin: string): void };

window.onmessage = async event => {
  const message = (event.data as { pluginMessage?: unknown } | undefined)?.pluginMessage;
  if (await handleVontaqFSFigmaUiMessage(message, {
    postMessage: reply => parent.postMessage({ pluginMessage: reply }, '*'),
  })) return;
  // Handle application-specific UI messages here.
};
