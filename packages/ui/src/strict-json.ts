/**
 * Parse JSON without accepting duplicate object keys.
 *
 * `JSON.parse` keeps the last value when an object contains the same key more
 * than once. That is unsafe at a workflow authoring boundary because the
 * bytes being reviewed, digested, and published no longer have one obvious
 * meaning. The scanner below walks the JSON grammar far enough to decode and
 * compare keys, then delegates value construction and the final syntax check
 * to the platform parser.
 */

/** Maximum nesting accepted by the workflow JSON boundary. */
export const A3S_FLOW_JSON_MAX_DEPTH = 256;

/** Parse one JSON value while rejecting duplicate keys at every object level. */
export function parseA3SFlowStrictJson(source: string): unknown {
  const scanner = new JsonKeyScanner(source);
  scanner.scan();
  return JSON.parse(source);
}

class JsonKeyScanner {
  private index = 0;

  public constructor(private readonly source: string) {}

  public scan(): void {
    this.skipWhitespace();
    this.scanValue(0);
    this.skipWhitespace();
    if (this.index !== this.source.length) {
      throw this.error('unexpected characters after the JSON value');
    }
  }

  private scanValue(depth: number): void {
    if (depth > A3S_FLOW_JSON_MAX_DEPTH) {
      throw this.error(
        `JSON nesting exceeds the maximum depth ${A3S_FLOW_JSON_MAX_DEPTH}`,
      );
    }
    this.skipWhitespace();
    const character = this.source[this.index];
    if (character === '"') {
      this.scanString();
      return;
    }
    if (character === '{') {
      this.scanObject(depth + 1);
      return;
    }
    if (character === '[') {
      this.scanArray(depth + 1);
      return;
    }
    if (character === 't') {
      this.scanLiteral('true');
      return;
    }
    if (character === 'f') {
      this.scanLiteral('false');
      return;
    }
    if (character === 'n') {
      this.scanLiteral('null');
      return;
    }
    if (character === '-' || (character !== undefined && /[0-9]/.test(character))) {
      this.scanNumber();
      return;
    }
    throw this.error('expected a JSON value');
  }

  private scanObject(depth: number): void {
    this.index += 1; // {
    this.skipWhitespace();
    const keys = new Set<string>();
    if (this.consume('}')) return;

    while (true) {
      this.skipWhitespace();
      if (this.source[this.index] !== '"') {
        throw this.error('expected a JSON object key');
      }
      const start = this.index;
      this.scanString();
      const rawKey = this.source.slice(start, this.index);
      let key: unknown;
      try {
        key = JSON.parse(rawKey);
      } catch {
        // The final JSON.parse below reports the complete syntax error. This
        // branch is only reachable for an invalid string token.
        throw this.error('invalid JSON object key');
      }
      if (typeof key !== 'string') {
        throw this.error('JSON object key is not a string');
      }
      if (keys.has(key)) {
        throw this.error(`duplicate JSON object key ${JSON.stringify(key)}`);
      }
      keys.add(key);
      this.skipWhitespace();
      this.expect(':');
      this.scanValue(depth);
      this.skipWhitespace();
      if (this.consume('}')) return;
      this.expect(',');
    }
  }

  private scanArray(depth: number): void {
    this.index += 1; // [
    this.skipWhitespace();
    if (this.consume(']')) return;
    while (true) {
      this.scanValue(depth);
      this.skipWhitespace();
      if (this.consume(']')) return;
      this.expect(',');
    }
  }

  private scanString(): void {
    this.expect('"');
    while (this.index < this.source.length) {
      const code = this.source.charCodeAt(this.index);
      this.index += 1;
      if (code === 0x22) return; // "
      if (code === 0x5c) {
        // Escaped quotes, reverse solidus, control escapes, and \uXXXX are
        // validated by JSON.parse; skipping one escaped code unit is enough
        // to keep the scanner aligned with the next structural character.
        if (this.index >= this.source.length) throw this.error('unterminated JSON escape');
        this.index += 1;
        continue;
      }
      if (code < 0x20) throw this.error('unescaped control character in JSON string');
    }
    throw this.error('unterminated JSON string');
  }

  private scanLiteral(literal: string): void {
    if (!this.source.startsWith(literal, this.index)) {
      throw this.error(`invalid JSON literal; expected ${literal}`);
    }
    this.index += literal.length;
  }

  private scanNumber(): void {
    // The platform parser owns number grammar and range semantics. Stop at a
    // structural delimiter so nested keys are still discovered correctly.
    while (this.index < this.source.length) {
      const character = this.source[this.index];
      if (character === ',' || character === ']' || character === '}' || /\s/.test(character)) {
        return;
      }
      this.index += 1;
    }
  }

  private skipWhitespace(): void {
    while (this.index < this.source.length && /\s/.test(this.source[this.index]!)) {
      this.index += 1;
    }
  }

  private consume(character: string): boolean {
    if (this.source[this.index] !== character) return false;
    this.index += 1;
    return true;
  }

  private expect(character: string): void {
    if (!this.consume(character)) {
      throw this.error(`expected ${JSON.stringify(character)}`);
    }
  }

  private error(message: string): SyntaxError {
    return new SyntaxError(`${message} at position ${this.index}`);
  }
}
