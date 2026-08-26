import { describe, expect, it } from 'vitest';
import {
  cleanTriggerValue,
  createTriggerDraft,
  isTriggerSchema,
  setTriggerValueAtPath,
  triggerValueAtPath,
  validateTriggerInput,
  type PlaygroundTriggerSchema,
} from './WorkflowPlayground.trigger';

describe('Workflow Playground trigger input contract', () => {
  const schema: PlaygroundTriggerSchema = {
    type: 'object',
    required: ['order_id', 'amount', 'customer', 'market'],
    properties: {
      order_id: { type: 'string', minLength: 3 },
      amount: { type: 'number', minimum: 1 },
      market: { type: 'string', enum: ['CN', 'DE'] },
      customer: {
        type: 'object',
        required: ['email'],
        properties: {
          email: { type: 'string', pattern: '^.+@.+$' },
          phone: { type: 'string' },
        },
      },
      items: {
        type: 'array',
        minItems: 1,
        items: {
          type: 'object',
          required: ['sku'],
          properties: { sku: { type: 'string' } },
        },
      },
    },
  };

  it('recognizes JSON Schema objects and rejects unsupported types', () => {
    expect(isTriggerSchema(schema)).toBe(true);
    expect(isTriggerSchema({ type: 'unsupported' })).toBe(false);
    expect(isTriggerSchema({ apiVersion: 'expression/v1' })).toBe(false);
    expect(isTriggerSchema({ properties: { id: { type: 'string' } } })).toBe(
      true,
    );
    expect(isTriggerSchema(null)).toBe(false);
    expect(isTriggerSchema({})).toBe(true);
    expect(isTriggerSchema({ type: ['string', 'null'] })).toBe(true);
    expect(isTriggerSchema({ additionalProperties: true })).toBe(true);
  });

  it('creates an editable draft with required nested fields', () => {
    const draft = createTriggerDraft(schema);
    expect(draft).toMatchObject({
      order_id: '',
      amount: '',
      customer: { email: '' },
      market: '',
    });
    expect(triggerValueAtPath(draft, ['customer', 'email'])).toBe('');
  });

  it('validates required, type, range, enum, pattern, and nested array rules', () => {
    const errors = validateTriggerInput(
      schema,
      {
        order_id: 'x',
        amount: 0,
        market: 'US',
        customer: { email: 'invalid' },
        items: [{ sku: '' }],
      },
      'en',
    );
    expect(errors.map(({ path }) => path)).toEqual(
      expect.arrayContaining([
        'order_id',
        'amount',
        'market',
        'customer.email',
        'items.0.sku',
      ]),
    );
  });

  it('updates nested values immutably and removes undefined placeholders', () => {
    const original = createTriggerDraft(schema);
    const updated = setTriggerValueAtPath(
      original,
      ['customer', 'email'],
      'ops@example.com',
    );
    expect(original).not.toBe(updated);
    expect(triggerValueAtPath(updated, ['customer', 'email'])).toBe(
      'ops@example.com',
    );
    expect(
      cleanTriggerValue({
        customer: { email: 'ops@example.com' },
        optional: undefined as never,
      }),
    ).toEqual({ customer: { email: 'ops@example.com' } });
  });

  it('validates schemas that omit type and schemas with union types', () => {
    const inferredObject: PlaygroundTriggerSchema = {
      required: ['name'],
      properties: { name: { type: 'string' } },
    };
    expect(validateTriggerInput(inferredObject, {}, 'en')).toEqual(
      expect.arrayContaining([expect.objectContaining({ path: 'name' })]),
    );

    const nullableNumber: PlaygroundTriggerSchema = {
      type: ['number', 'null'],
      minimum: 10,
    };
    expect(validateTriggerInput(nullableNumber, null, 'en')).toEqual([]);
    expect(validateTriggerInput(nullableNumber, 4, 'en')).toEqual(
      expect.arrayContaining([expect.objectContaining({ path: '' })]),
    );
    expect(validateTriggerInput(nullableNumber, '4', 'en')).toEqual(
      expect.arrayContaining([expect.objectContaining({ path: '' })]),
    );
  });

  it('accepts deep enum and const values and enforces collection constraints', () => {
    const constrained: PlaygroundTriggerSchema = {
      type: 'object',
      minProperties: 2,
      properties: {
        tags: {
          type: 'array',
          uniqueItems: true,
          minItems: 2,
          items: { type: 'string' },
        },
        mode: { type: 'object', enum: [{ kind: 'safe' }] },
      },
      required: ['tags', 'mode'],
    };
    const errors = validateTriggerInput(
      constrained,
      {
        tags: ['a', 'a'],
        mode: { kind: 'safe' },
      },
      'en',
    );
    expect(errors.map(({ path }) => path)).toContain('tags');
    expect(
      validateTriggerInput(
        { type: 'object', properties: { value: { const: { ok: true } } } },
        { value: { ok: true } },
        'en',
      ),
    ).toEqual([]);
  });
});
