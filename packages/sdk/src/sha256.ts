const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

const INITIAL = new Uint32Array([
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
  0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
]);

function rotr(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits));
}

export class IncrementalSha256 {
  private readonly state = new Uint32Array(INITIAL);
  private readonly buffer = new Uint8Array(64);
  private readonly words = new Uint32Array(64);
  private bufferLength = 0;
  private bytesHashed = 0;
  private finalized = false;

  update(data: Uint8Array): void {
    if (this.finalized) throw new Error('SHA-256 has already been finalized.');
    this.bytesHashed += data.byteLength;
    let offset = 0;
    if (this.bufferLength > 0) {
      const take = Math.min(64 - this.bufferLength, data.byteLength);
      this.buffer.set(data.subarray(0, take), this.bufferLength);
      this.bufferLength += take;
      offset += take;
      if (this.bufferLength === 64) {
        this.processBlock(this.buffer);
        this.bufferLength = 0;
      }
    }
    while (offset + 64 <= data.byteLength) {
      this.processBlock(data.subarray(offset, offset + 64));
      offset += 64;
    }
    if (offset < data.byteLength) {
      const rest = data.subarray(offset);
      this.buffer.set(rest, 0);
      this.bufferLength = rest.byteLength;
    }
  }

  digestHex(): string {
    if (this.finalized) throw new Error('SHA-256 has already been finalized.');
    this.finalized = true;
    const bitLength = BigInt(this.bytesHashed) * 8n;
    this.buffer[this.bufferLength++] = 0x80;
    if (this.bufferLength > 56) {
      this.buffer.fill(0, this.bufferLength);
      this.processBlock(this.buffer);
      this.bufferLength = 0;
    }
    this.buffer.fill(0, this.bufferLength, 56);
    for (let index = 0; index < 8; index += 1) {
      this.buffer[63 - index] = Number((bitLength >> BigInt(index * 8)) & 0xffn);
    }
    this.processBlock(this.buffer);
    return Array.from(this.state, value => value.toString(16).padStart(8, '0')).join('');
  }

  private processBlock(block: Uint8Array): void {
    const w = this.words;
    for (let index = 0; index < 16; index += 1) {
      const offset = index * 4;
      w[index] = (((block[offset] ?? 0) << 24) | ((block[offset + 1] ?? 0) << 16) | ((block[offset + 2] ?? 0) << 8) | (block[offset + 3] ?? 0)) >>> 0;
    }
    for (let index = 16; index < 64; index += 1) {
      const a = w[index - 15] ?? 0;
      const b = w[index - 2] ?? 0;
      const s0 = rotr(a, 7) ^ rotr(a, 18) ^ (a >>> 3);
      const s1 = rotr(b, 17) ^ rotr(b, 19) ^ (b >>> 10);
      w[index] = ((w[index - 16] ?? 0) + s0 + (w[index - 7] ?? 0) + s1) >>> 0;
    }

    let a = this.state[0] ?? 0;
    let b = this.state[1] ?? 0;
    let c = this.state[2] ?? 0;
    let d = this.state[3] ?? 0;
    let e = this.state[4] ?? 0;
    let f = this.state[5] ?? 0;
    let g = this.state[6] ?? 0;
    let h = this.state[7] ?? 0;

    for (let index = 0; index < 64; index += 1) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const temp1 = (h + s1 + ch + (K[index] ?? 0) + (w[index] ?? 0)) >>> 0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (s0 + maj) >>> 0;
      h = g; g = f; f = e; e = (d + temp1) >>> 0; d = c; c = b; b = a; a = (temp1 + temp2) >>> 0;
    }

    this.state[0] = ((this.state[0] ?? 0) + a) >>> 0;
    this.state[1] = ((this.state[1] ?? 0) + b) >>> 0;
    this.state[2] = ((this.state[2] ?? 0) + c) >>> 0;
    this.state[3] = ((this.state[3] ?? 0) + d) >>> 0;
    this.state[4] = ((this.state[4] ?? 0) + e) >>> 0;
    this.state[5] = ((this.state[5] ?? 0) + f) >>> 0;
    this.state[6] = ((this.state[6] ?? 0) + g) >>> 0;
    this.state[7] = ((this.state[7] ?? 0) + h) >>> 0;
  }
}
