import { describe, expect, it } from 'vitest';
import { tokenizeCode } from './CodeExample.tokens';

describe('CodeExample tokenizer', () => {
  it('distinguishes JSON properties, values, and literals', () => {
    const tokens = tokenizeCode(
      '{"enabled": true, "name": "checkout", "retries": 3}',
      'json',
    );

    expect(tokens).toEqual(
      expect.arrayContaining([
        { kind: 'property', value: '"enabled"' },
        { kind: 'boolean', value: 'true' },
        { kind: 'string', value: '"checkout"' },
        { kind: 'number', value: '3' },
      ]),
    );
  });

  it('marks Bash commands and options without changing their values', () => {
    const tokens = tokenizeCode(
      'a3s-flow validate workflow.json --pretty',
      'bash',
    );

    expect(tokens).toEqual(
      expect.arrayContaining([
        { kind: 'command', value: 'a3s-flow' },
        { kind: 'keyword', value: 'validate' },
        { kind: 'parameter', value: '--pretty' },
      ]),
    );
    expect(tokens.map(({ value }) => value).join('')).toBe(
      'a3s-flow validate workflow.json --pretty',
    );
  });

  it('highlights Skill and workflow references in prompt text', () => {
    const tokens = tokenizeCode(
      'Use $a3s-flow to add the "flow.start" node to workflow.json.',
      'text',
    );

    expect(tokens).toEqual(
      expect.arrayContaining([
        { kind: 'variable', value: '$a3s-flow' },
        { kind: 'string', value: '"flow.start"' },
        { kind: 'file', value: 'workflow.json' },
      ]),
    );
  });
});
