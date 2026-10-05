export type PreviewFileInfo = {
  path: string;
  contentType?: string;
  formatId?: string;
  opaque?: boolean;
};

export type FilePreviewMode = 'text' | 'json' | 'image' | 'metadata-only';

const SAFE_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp']);

/**
 * Desktop preview policy is deliberately metadata-driven and conservative.
 * Unknown/custom/encrypted content remains metadata-only; this helper never
 * parses payload bytes to guess a proprietary format.
 */
export function filePreviewMode(file: PreviewFileInfo): FilePreviewMode {
  if (file.opaque === true) return 'metadata-only';
  const contentType = file.contentType?.toLowerCase();
  if (contentType === 'application/json' || contentType?.endsWith('+json')) return 'json';
  if (contentType?.startsWith('text/')) return 'text';
  if (contentType && SAFE_IMAGE_TYPES.has(contentType)) return 'image';
  return 'metadata-only';
}
