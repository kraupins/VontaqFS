import { VontaqFS } from '@vontaq/fs';
import { bindFigmaDocument, createFigmaConnectOptions } from '@vontaq/fs/figma';

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
  notify(message: string): void;
  closePlugin(): void;
};

async function main(): Promise<void> {
  const client = await VontaqFS.connect({
    ...createFigmaConnectOptions(figma, { displayName: 'VontaqFS Example' }),
    onPairingRequired: () => figma.notify('Approve VontaqFS access in the VontaqFS window.'),
  });

  const binding = await bindFigmaDocument(figma);
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
  await client.close();
}

main()
  .catch(error => figma.notify(error instanceof Error ? error.message : String(error)))
  .finally(() => figma.closePlugin());
