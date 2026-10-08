const REPLACEMENT_CODE_POINT = 0xfffd;

export interface PortableUtf8DecodeOptions {
  fatal?: boolean;
}

export function utf8Encode(value: string): Uint8Array {
  if (typeof value !== 'string') throw new TypeError('utf8Encode() requires a string.');
  const output = new Uint8Array(value.length * 3);
  let write = 0;
  for (let index = 0; index < value.length; index += 1) {
    const first = value.charCodeAt(index);
    let codePoint = first;
    if (first >= 0xd800 && first <= 0xdbff) {
      const second = value.charCodeAt(index + 1);
      if (second >= 0xdc00 && second <= 0xdfff) {
        codePoint = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
        index += 1;
      } else {
        codePoint = REPLACEMENT_CODE_POINT;
      }
    } else if (first >= 0xdc00 && first <= 0xdfff) {
      codePoint = REPLACEMENT_CODE_POINT;
    }

    if (codePoint <= 0x7f) {
      output[write++] = codePoint;
    } else if (codePoint <= 0x7ff) {
      output[write++] = 0xc0 | (codePoint >> 6);
      output[write++] = 0x80 | (codePoint & 0x3f);
    } else if (codePoint <= 0xffff) {
      output[write++] = 0xe0 | (codePoint >> 12);
      output[write++] = 0x80 | ((codePoint >> 6) & 0x3f);
      output[write++] = 0x80 | (codePoint & 0x3f);
    } else {
      output[write++] = 0xf0 | (codePoint >> 18);
      output[write++] = 0x80 | ((codePoint >> 12) & 0x3f);
      output[write++] = 0x80 | ((codePoint >> 6) & 0x3f);
      output[write++] = 0x80 | (codePoint & 0x3f);
    }
  }
  return output.slice(0, write);
}

export function utf8Decode(bytes: Uint8Array, options: PortableUtf8DecodeOptions = {}): string {
  const decoder = new PortableUtf8Decoder(options);
  return decoder.decode(bytes);
}

export class PortableUtf8Decoder {
  private pending = new Uint8Array(0);
  private readonly fatal: boolean;

  constructor(options: PortableUtf8DecodeOptions = {}) {
    this.fatal = options.fatal ?? false;
  }

  decode(input: Uint8Array = new Uint8Array(0), options: { stream?: boolean } = {}): string {
    if (!(input instanceof Uint8Array)) throw new TypeError('UTF-8 decoder input must be Uint8Array.');
    const bytes = this.pending.byteLength === 0 ? input : concatenate(this.pending, input);
    this.pending = new Uint8Array(0);
    const output: string[] = [];
    let index = 0;

    const reject = (advance = 1): void => {
      if (this.fatal) throw new TypeError('The encoded data was not valid UTF-8.');
      output.push(String.fromCodePoint(REPLACEMENT_CODE_POINT));
      index += advance;
    };

    while (index < bytes.byteLength) {
      const first = bytes[index]!;
      if (first <= 0x7f) {
        output.push(String.fromCodePoint(first));
        index += 1;
        continue;
      }

      let length: 2 | 3 | 4;
      let minSecond = 0x80;
      let maxSecond = 0xbf;
      if (first >= 0xc2 && first <= 0xdf) {
        length = 2;
      } else if (first >= 0xe0 && first <= 0xef) {
        length = 3;
        if (first === 0xe0) minSecond = 0xa0;
        if (first === 0xed) maxSecond = 0x9f;
      } else if (first >= 0xf0 && first <= 0xf4) {
        length = 4;
        if (first === 0xf0) minSecond = 0x90;
        if (first === 0xf4) maxSecond = 0x8f;
      } else {
        reject();
        continue;
      }

      if (index + length > bytes.byteLength) {
        if (options.stream) {
          this.pending = bytes.slice(index);
          break;
        }
        reject(bytes.byteLength - index);
        continue;
      }

      const second = bytes[index + 1]!;
      if (second < minSecond || second > maxSecond) {
        reject();
        continue;
      }
      let valid = true;
      for (let offset = 2; offset < length; offset += 1) {
        const continuation = bytes[index + offset]!;
        if (continuation < 0x80 || continuation > 0xbf) {
          valid = false;
          break;
        }
      }
      if (!valid) {
        reject();
        continue;
      }

      let codePoint: number;
      if (length === 2) {
        codePoint = ((first & 0x1f) << 6) | (second & 0x3f);
      } else if (length === 3) {
        codePoint = ((first & 0x0f) << 12) | ((second & 0x3f) << 6) | (bytes[index + 2]! & 0x3f);
      } else {
        codePoint = ((first & 0x07) << 18) | ((second & 0x3f) << 12) | ((bytes[index + 2]! & 0x3f) << 6) | (bytes[index + 3]! & 0x3f);
      }
      output.push(String.fromCodePoint(codePoint));
      index += length;
    }

    return output.join('');
  }
}

function concatenate(a: Uint8Array, b: Uint8Array): Uint8Array {
  const output = new Uint8Array(a.byteLength + b.byteLength);
  output.set(a);
  output.set(b, a.byteLength);
  return output;
}
