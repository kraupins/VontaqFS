import { VontaqFS } from '@vontaq/fs';
import { bindFigmaDocument, createFigmaConnectOptions, createFigmaMainHostAdapter } from '@vontaq/fs/figma';

declare const __html__: string;
declare const figma: {
  pluginId?: string;
  widgetId?: string;
  clientStorage: {
    getAsync(key: string): Promise<unknown>;
    setAsync(key: string, value: unknown): Promise<void>;
    deleteAsync?(key: string): Promise<void>;
  };
  root: {
    name: string;
    getPluginData(key: string): string;
    setPluginData(key: string, value: string): void;
  };
  ui: {
    postMessage(message: unknown): void;
    onmessage: ((message: unknown) => void | Promise<void>) | undefined;
  };
  showUI(html: string, options?: { visible?: boolean; width?: number; height?: number }): void;
  notify(message: string): void;
  closePlugin(): void;
};

async function main(): Promise<void> {
  figma.showUI(__html__, { visible: false, width: 1, height: 1 });

  const host = createFigmaMainHostAdapter({
    postMessage: message => figma.ui.postMessage(message),
  });
  figma.ui.onmessage = async message => {
    if (host.handleUiMessage(message)) return;
    // Handle application-specific UI messages here.
  };

  try {
    await host.ready();
    const client = await VontaqFS.connect({
      ...createFigmaConnectOptions(figma, { displayName: 'VontaqFS Example', host }),
      onPairingRequired: () => figma.notify('Approve VontaqFS access in the VontaqFS window.'),
    });

    try {
      const binding = await bindFigmaDocument(figma, { secureRandom: host.secureRandom });
      const documentSpace = await client.openSpace({
        key: `figma-document:${binding.id}`,
        displayName: binding.displayName,
        storageClass: 'persistent',
      });

      await documentSpace.files.writeJSON('/example.json', {
        savedAt: new Date().toISOString(),
        documentBindingId: binding.id,
      });

      const restored = await documentSpace.files.readJSON<{ savedAt: string }>('/example.json');
      figma.notify(`VontaqFS restored data saved at ${restored.savedAt}`);
    } finally {
      await client.close();
    }
  } finally {
    host.close();
  }
}

main()
  .catch(error => figma.notify(error instanceof Error ? error.message : String(error)))
  .finally(() => figma.closePlugin());
