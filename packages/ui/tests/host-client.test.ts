import {
  FlowHostClient,
  HostClientError,
  canonicalJson,
  issueApprovalRecord,
  parseHostModeSearch,
  sha256Digest,
} from '../src/integrations/host-client';

describe('FlowHostClient', () => {
  it('fail-closes without a base URL', () => {
    expect(() => new FlowHostClient({ baseUrl: '  ' })).toThrow(HostClientError);
  });

  it('parses host mode query params fail-closed', () => {
    expect(parseHostModeSearch('')).toBeNull();
    expect(parseHostModeSearch('?example=demo')).toBeNull();
    expect(() => parseHostModeSearch('?host=http://127.0.0.1:8080')).toThrow(
      HostClientError,
    );
    expect(parseHostModeSearch('?host=http://127.0.0.1:8080&runId=run-1')).toEqual(
      {
        host: 'http://127.0.0.1:8080',
        runId: 'run-1',
        tenantId: 'tenant-local-validation',
        principalRef: 'reviewer@local',
      },
    );
  });

  it('opens a canvas through GET /proposal', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({
          schema_version: 'host.flow-http-proposal.v1',
          run_id: 'run-1',
          proposal_digest: 'sha256:abc',
          flow_dsl: { kind: 'app' },
          execution_digest: 'deadbeef',
          approval_hook_waiting: true,
          approval_token: 'tok',
          proposal: {
            plan: {
              schema_version: 'host.agent-plan.v2',
              steps: [
                {
                  step_id: 'step-001',
                  agent_id: 'frontend-developer',
                  objective: 'review',
                  capabilities: ['read'],
                  depends_on: [],
                },
              ],
              edges: [],
              execution_order: ['step-001'],
            },
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const canvas = await client.openCanvas('run-1');
    expect(canvas.proposal_digest).toBe('sha256:abc');
    expect(canvas.save.path).toBe('/v1/runs/run-1/plan-edits');
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs/run-1/proposal',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('posts plan-edits from the canvas authority', async () => {
    const fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/proposal')) {
        return new Response(
          JSON.stringify({
            run_id: 'run-1',
            proposal_digest: 'sha256:abc',
            proposal: {
              plan: {
                schema_version: 'host.agent-plan.v2',
                steps: [
                  {
                    step_id: 'step-001',
                    agent_id: 'frontend-developer',
                    objective: 'review',
                    capabilities: ['read'],
                    depends_on: [],
                  },
                ],
                edges: [],
                execution_order: ['step-001'],
              },
            },
          }),
          { status: 200 },
        );
      }
      expect(init?.method).toBe('POST');
      const body = JSON.parse(String(init?.body)) as {
        schema_version: string;
        edit_revision: number;
      };
      expect(body.schema_version).toBe('host.plan-edit-state.v1');
      expect(body.edit_revision).toBe(1);
      return new Response(
        JSON.stringify({
          schema_version: 'host.flow-http-plan-edit.v1',
          decision: { status: 'UNCHANGED' },
        }),
        { status: 200 },
      );
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const canvas = await client.openCanvas('run-1');
    const result = await client.saveCanvas(canvas, 1);
    expect(result.status).toBe(200);
    expect(result.json.decision).toMatchObject({ status: 'UNCHANGED' });
  });

  it('issues approval records with a stable digest', async () => {
    const approvedAt = new Date('2026-09-15T12:00:00.000Z');
    const record = await issueApprovalRecord({
      proposalDigest: 'sha256:abc',
      tenantId: 'tenant-a',
      principalRef: 'reviewer@local',
      approved: true,
      approvedAt,
      ttlMs: 30 * 60 * 1000,
    });
    expect(record.schema_version).toBe('host.flow-approval.v1');
    expect(record.expires_at_utc).toBe('2026-09-15T12:30:00Z');
    const { approval_digest: _omit, ...body } = record;
    expect(record.approval_digest).toBe(await sha256Digest(body));
    expect(canonicalJson({ z: 1, a: 2 })).toBe('{"a":2,"z":1}');
  });
});
