export type CodeExampleLanguage = 'bash' | 'json' | 'text';

export type CodeTokenKind =
  | 'boolean'
  | 'command'
  | 'comment'
  | 'file'
  | 'keyword'
  | 'number'
  | 'parameter'
  | 'plain'
  | 'property'
  | 'punctuation'
  | 'string'
  | 'variable';

export type CodeToken = {
  kind: CodeTokenKind;
  value: string;
};

const JSON_TOKEN_PATTERN =
  /"(?:\\.|[^"\\])*"(?=\s*:)|"(?:\\.|[^"\\])*"|-?\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b|\b(?:true|false|null)\b|[{}[\],:]/giu;
const BASH_TOKEN_PATTERN =
  /"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|#[^\n]*|\$[\w.-]+|--?[\w-]+|\b(?:a3s-flow|node|new|validate|compile|digest)\b/gu;
const TEXT_TOKEN_PATTERN = /\$[\w.-]+|"(?:\\.|[^"\\])*"|[\w.-]+\.json\b/gu;

function jsonTokenKind(value: string): CodeTokenKind {
  if (value.startsWith('"')) return 'string';
  if (/^(?:true|false|null)$/u.test(value)) return 'boolean';
  if (/^-?\d/u.test(value)) return 'number';
  return 'punctuation';
}

function bashTokenKind(value: string): CodeTokenKind {
  if (value.startsWith('#')) return 'comment';
  if (value.startsWith('"') || value.startsWith("'")) return 'string';
  if (value.startsWith('$')) return 'variable';
  if (value.startsWith('-')) return 'parameter';
  if (value === 'a3s-flow') return 'command';
  return 'keyword';
}

function textTokenKind(value: string): CodeTokenKind {
  if (value.startsWith('$')) return 'variable';
  if (value.startsWith('"')) return 'string';
  return 'file';
}

function tokenizeWithPattern(
  source: string,
  pattern: RegExp,
  classify: (value: string) => CodeTokenKind,
): CodeToken[] {
  const tokens: CodeToken[] = [];
  let cursor = 0;
  pattern.lastIndex = 0;
  for (const match of source.matchAll(pattern)) {
    const index = match.index;
    if (index > cursor) {
      tokens.push({ kind: 'plain', value: source.slice(cursor, index) });
    }
    tokens.push({ kind: classify(match[0]), value: match[0] });
    cursor = index + match[0].length;
  }
  if (cursor < source.length) {
    tokens.push({ kind: 'plain', value: source.slice(cursor) });
  }
  return tokens;
}

export function tokenizeCode(
  source: string,
  language: CodeExampleLanguage,
): CodeToken[] {
  if (language === 'json') {
    const tokens = tokenizeWithPattern(
      source,
      JSON_TOKEN_PATTERN,
      jsonTokenKind,
    );
    return tokens.map((token, index) => {
      if (
        token.kind === 'string' &&
        tokens[index + 1]?.kind === 'punctuation' &&
        tokens[index + 1]?.value === ':'
      ) {
        return { ...token, kind: 'property' };
      }
      return token;
    });
  }
  if (language === 'bash') {
    return tokenizeWithPattern(source, BASH_TOKEN_PATTERN, bashTokenKind);
  }
  return tokenizeWithPattern(source, TEXT_TOKEN_PATTERN, textTokenKind);
}
