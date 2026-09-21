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
    // host alone is the run-history picker state (no run selected yet).
    expect(parseHostModeSearch('?host=http://127.0.0.1:8080')).toEqual({
      host: 'http://127.0.0.1:8080',
      runId: '',
      tenantId: 'tenant-local-validation',
      principalRef: 'reviewer@local',
    });
    // runId alone can never be resolved -- still an error.
    expect(() => parseHostModeSearch('?runId=run-1')).toThrow(HostClientError);
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

  it('lists runs through GET /v1/runs', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({
          schema_version: 'host.flow-http-runs.v1',
          runs: [
            { run_id: 'run-1', status: 'Suspended', task_text: 'review the docs' },
            { run_id: 'run-2', status: 'Running', task_text: 'fix the bug' },
          ],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const result = await client.listRuns();
    expect(result.schema_version).toBe('host.flow-http-runs.v1');
    expect(result.runs).toHaveLength(2);
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('starts a new run through POST /v1/runs', async () => {
    const fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body ?? '{}'));
      expect(body).toMatchObject({
        run_id: 'run-new-1',
        task_text: 'survey AI development in the US, China, Japan, and Korea',
        permission_ceiling: ['read'],
        approval_token: 'tok-new-1',
      });
      return new Response(
        JSON.stringify({ run_id: 'run-new-1', status: 'running' }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.startRun('run-new-1', {
      task_text: 'survey AI development in the US, China, Japan, and Korea',
      permission_ceiling: ['read'],
      approval_token: 'tok-new-1',
    });
    expect(status).toBe(200);
    expect(json.run_id).toBe('run-new-1');
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs',
      expect.objectContaining({ method: 'POST' }),
    );
  });

  it('startRun surfaces a run conflict as a non-throwing status/json pair', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({ error: 'run conflict: workflow input differs' }),
        { status: 409, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.startRun('run-existing', {
      task_text: 'a different task',
      permission_ceiling: ['read'],
      approval_token: 'tok',
    });
    expect(status).toBe(409);
    expect(json.error).toContain('conflict');
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
