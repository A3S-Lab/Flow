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

  it('reads the run snapshot through GET /v1/runs/{id}', async () => {
    const snapshot = {
      run_id: 'run 1',
      status: 'running',
      steps: { 'agent:step-001': { status: 'running' } },
    };
    const fetchImpl = vi.fn(async () =>
      new Response(JSON.stringify(snapshot), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(await client.getRun('run 1')).toEqual(snapshot);
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs/run%201',
      expect.objectContaining({ method: 'GET' }),
    );
    await expect(client.getRun('  ')).rejects.toThrow('runId is required');
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

  it('startRun defaults json to {} when a non-2xx reply has an empty body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 500 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.startRun('run-1', {
      task_text: 'x',
      permission_ceiling: ['read'],
      approval_token: 'tok',
    });
    expect(status).toBe(500);
    expect(json).toEqual({});
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

  it('gets recovery failure context through GET /failure', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({
          schema_version: 'host.flow-recovery-request.v1',
          run_id: 'run-1',
          hook_id: 'recovery:step-001.0',
          token: 'tok-recovery',
          metadata: {
            step_id: 'step-001',
            step_name: 'orchestrator.agent_step',
            attempt: 0,
            error: 'boom',
            original_step: { step_id: 'step-001', agent_id: 'a', version: '1.0.0' },
            proposal_digest: 'sha256:abc',
            completed_step_outputs: [],
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.getRecoveryFailure('run-1');
    expect(status).toBe(200);
    expect(json.hook_id).toBe('recovery:step-001.0');
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs/run-1/failure',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('getRecoveryFailure surfaces a 404 (no open hook) as a non-throwing status/json pair', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({ error: 'NOT_FOUND: run has no open recovery hook' }),
        { status: 404, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.getRecoveryFailure('run-1');
    expect(status).toBe(404);
    expect(json.error).toContain('NOT_FOUND');
  });

  it('getRecoveryFailure defaults json to {} when a non-2xx reply has an empty body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 500 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.getRecoveryFailure('run-1');
    expect(status).toBe(500);
    expect(json).toEqual({});
  });

  it('posts a resume-after-failure edit through POST /resume-after-failure', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({
          schema_version: 'host.flow-recovery-resume.v1',
          hook_id: 'recovery:step-001.0',
          snapshot: { status: 'Running' },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postResumeAfterFailure('run-1', {
      objective: 'try again with a different angle',
    });
    expect(status).toBe(200);
    expect(json.hook_id).toBe('recovery:step-001.0');
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs/run-1/resume-after-failure',
      expect.objectContaining({ method: 'POST' }),
    );
  });

  it('postResumeAfterFailure surfaces a rejected edit as a non-throwing status/json pair', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({
          error: 'REJECTED: UNKNOWN_AGENT_REF: agent ghost@9.9.9 is not in the plan',
        }),
        { status: 400, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postResumeAfterFailure('run-1', {
      agent_id: 'ghost',
      version: '9.9.9',
    });
    expect(status).toBe(400);
    expect(json.error).toContain('REJECTED');
  });

  it('postResumeAfterFailure defaults json to {} when a non-2xx reply has an empty body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 500 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postResumeAfterFailure('run-1', {});
    expect(status).toBe(500);
    expect(json).toEqual({});
  });

  it('getProgressStream yields each host.compose-progress.v1 NDJSON frame in order', async () => {
    const frame1 = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-16T04:00:00+00:00',
      step_index: 1,
      total: 2,
      agent_id: 'frontend-developer',
      status: 'draft',
      progress_token: 'orchestrator.compose_plan|i=1|n=2|a=frontend-developer',
      progress: 1.0,
    };
    const frame2 = { ...frame1, step_index: 2, agent_id: 'react-specialist' };
    const fetchImpl = vi.fn(async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify(frame1)}\n`),
            );
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify(frame2)}\n`),
            );
            controller.close();
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/x-ndjson' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const frame of client.getProgressStream('run-1')) {
      received.push(frame);
    }
    expect(received).toEqual([frame1, frame2]);
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs/run-1/progress',
      expect.objectContaining({
        headers: { Accept: 'application/x-ndjson' },
      }),
    );
  });

  it('getProgressStream keeps planning-phase fields and drops unknown ones', async () => {
    const phase = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-28T08:00:40+00:00',
      step_index: 0,
      total: null,
      agent_id: '',
      status: 'tool',
      progress_token: 'orchestrator.compose_plan|phase=tool',
      progress: 0,
      kind: 'phase',
      phase: 'tool',
      tool: 'shortlist',
      model_calls: 3,
      generated_chars: 120,
      elapsed_ms: 17900,
    };
    const fetchImpl = vi.fn(
      async () =>
        new Response(`${JSON.stringify({ ...phase, surprise: 'x' })}\n`, {
          status: 200,
          headers: { 'Content-Type': 'application/x-ndjson' },
        }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const frame of client.getProgressStream('run-1')) {
      received.push(frame);
    }
    expect(received).toEqual([phase]);
  });

  it('getProgressStream appends ?follow=true when requested', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(new ReadableStream({ start: (c) => c.close() }), {
        status: 200,
      }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const frame of client.getProgressStream('run-1', {
      follow: true,
    })) {
      received.push(frame);
    }
    expect(received).toEqual([]);
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/runs/run-1/progress?follow=true',
      expect.anything(),
    );
  });

  it('getProgressStream ends without throwing on a 404 (no host support / nothing buffered)', async () => {
    const fetchImpl = vi.fn(
      async () => new Response('not found', { status: 404 }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const frame of client.getProgressStream('run-1')) {
      received.push(frame);
    }
    expect(received).toEqual([]);
  });

  it('getProgressStream skips a malformed line without aborting the rest of the stream', async () => {
    const frame = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-16T04:00:00+00:00',
      step_index: 1,
      total: null,
      agent_id: 'frontend-developer',
      status: 'draft',
      progress_token: 'orchestrator.compose_plan|i=1|n=1|a=frontend-developer',
      progress: 1.0,
    };
    const fetchImpl = vi.fn(async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(new TextEncoder().encode('not json\n'));
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify(frame)}\n`),
            );
            controller.close();
          },
        }),
        { status: 200 },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const f of client.getProgressStream('run-1')) {
      received.push(f);
    }
    expect(received).toEqual([frame]);
  });

  it('getProgressStream skips a line missing a required field (parses but fails validation)', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(
              new TextEncoder().encode(`${JSON.stringify({ schema_version: 'host.compose-progress.v1' })}\n`),
            );
            controller.enqueue(new TextEncoder().encode('"just a string"\n'));
            controller.close();
          },
        }),
        { status: 200 },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const f of client.getProgressStream('run-1')) {
      received.push(f);
    }
    expect(received).toEqual([]);
  });

  it('getProgressStream defaults optional fields that are absent from a well-formed frame', async () => {
    const minimal = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      step_index: 1,
      agent_id: 'frontend-developer',
    };
    const fetchImpl = vi.fn(async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(new TextEncoder().encode(`${JSON.stringify(minimal)}\n`));
            controller.close();
          },
        }),
        { status: 200 },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const f of client.getProgressStream('run-1')) {
      received.push(f);
    }
    expect(received).toEqual([
      {
        schema_version: 'host.compose-progress.v1',
        flow_run_id: 'run-1',
        observed_at_utc: '',
        step_index: 1,
        total: null,
        agent_id: 'frontend-developer',
        status: '',
        progress_token: '',
        progress: 0,
      },
    ]);
  });

  it('getProgressStream yields a trailing frame with no terminating newline', async () => {
    const frame = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-16T04:00:00+00:00',
      step_index: 1,
      total: 1,
      agent_id: 'frontend-developer',
      status: 'draft',
      progress_token: 'orchestrator.compose_plan|i=1|n=1|a=frontend-developer',
      progress: 1.0,
    };
    const fetchImpl = vi.fn(async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            // No trailing '\n' -- only flushed once the reader reports `done`.
            controller.enqueue(new TextEncoder().encode(JSON.stringify(frame)));
            controller.close();
          },
        }),
        { status: 200 },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const f of client.getProgressStream('run-1')) {
      received.push(f);
    }
    expect(received).toEqual([frame]);
  });

  it('getProgressStream throws synchronously on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(async () => {
      for await (const _f of client.getProgressStream('  ')) {
        // never reached
      }
    }).rejects.toThrow(HostClientError);
  });

  it('getProgressStream ends fail-soft when the response has no body', async () => {
    const fetchImpl = vi.fn(
      async () =>
        ({ ok: true, body: null }) as unknown as Response,
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const f of client.getProgressStream('run-1')) {
      received.push(f);
    }
    expect(received).toEqual([]);
  });

  it('getProgressStream ends fail-soft when the stream errors mid-read', async () => {
    const frame = {
      schema_version: 'host.compose-progress.v1',
      flow_run_id: 'run-1',
      observed_at_utc: '2026-09-16T04:00:00+00:00',
      step_index: 1,
      total: 2,
      agent_id: 'frontend-developer',
      status: 'draft',
      progress_token: 'orchestrator.compose_plan|i=1|n=2|a=frontend-developer',
      progress: 1.0,
    };
    let controllerRef!: ReadableStreamDefaultController<Uint8Array>;
    const fetchImpl = vi.fn(async () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controllerRef = controller;
            controller.enqueue(new TextEncoder().encode(`${JSON.stringify(frame)}\n`));
          },
        }),
        { status: 200 },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received: unknown[] = [];
    await expect(
      (async () => {
        for await (const f of client.getProgressStream('run-1')) {
          received.push(f);
          // Error the stream only after the first frame has already been
          // read and yielded, to exercise the mid-loop `reader.read()`
          // rejection path rather than an error raised before any read.
          controllerRef.error(new Error('connection reset'));
        }
      })(),
    ).resolves.toBeUndefined();
    expect(received).toEqual([frame]);
  });

  it('getProgressStream ends fail-soft when fetch itself throws (network failure)', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('network error');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const received = [];
    for await (const f of client.getProgressStream('run-1')) {
      received.push(f);
    }
    expect(received).toEqual([]);
  });

  it('getProposal throws on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.getProposal('  ')).rejects.toThrow(HostClientError);
  });

  it('getProposal throws when the response body is not an object', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(JSON.stringify('just a string'), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(client.getProposal('run-1')).rejects.toThrow(
      /proposal response is empty/,
    );
  });

  it('request() wraps a network failure (fetch throws) as a HostClientError', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('fetch failed');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(client.listRuns()).rejects.toThrow(/host request failed/);
  });

  it('request() stringifies a non-Error thrown value in its failure message', async () => {
    const fetchImpl = vi.fn(async () => {
      // eslint-disable-next-line @typescript-eslint/no-throw-literal
      throw 'connection refused';
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(client.listRuns()).rejects.toThrow(/connection refused/);
  });

  it('request() falls back to {error: text} when the error body is not JSON', async () => {
    const fetchImpl = vi.fn(
      async () => new Response('internal error, not json', { status: 500 }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.getRecoveryFailure('run-1');
    expect(status).toBe(500);
    expect(json).toEqual({ error: 'internal error, not json' });
  });

  it('rejects a path that is not absolute (defensive internal invariant)', () => {
    const client = new FlowHostClient({ baseUrl: 'http://127.0.0.1:9' });
    expect(() => (client as unknown as { url(p: string): string }).url('relative')).toThrow(
      /path must be absolute/,
    );
  });

  it('postPlanEdits throws on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.postPlanEdits('  ', {})).rejects.toThrow(HostClientError);
  });

  it('postPlanEdits rethrows a genuine network failure instead of swallowing it', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('offline');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(client.postPlanEdits('run-1', {})).rejects.toThrow(
      /host request failed/,
    );
  });

  it('postPlanEdits defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postPlanEdits('run-1', {});
    expect(status).toBe(200);
    expect(json).toEqual({});
  });

  it('postPlanEdits surfaces a rejected edit as a non-throwing status/json pair', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(
          JSON.stringify({ error: 'REJECTED: structural check failed' }),
          { status: 400, headers: { 'Content-Type': 'application/json' } },
        ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postPlanEdits('run-1', {});
    expect(status).toBe(400);
    expect(json.error).toContain('REJECTED');
  });

  it('postPlanEdits defaults json to {} when a non-2xx reply has an empty body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 500 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postPlanEdits('run-1', {});
    expect(status).toBe(500);
    expect(json).toEqual({});
  });

  it('posts a copilot instruction and returns suggested steps', async () => {
    const fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://127.0.0.1:9/v1/runs/run-1/copilot');
      expect(JSON.parse(String(init?.body))).toEqual({
        instruction: 'add a review step',
      });
      return new Response(
        JSON.stringify({
          message: 'Added a review step.',
          suggested_steps: [{ step_id: 'step-2' }],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postCopilot('run-1', {
      instruction: 'add a review step',
    });
    expect(status).toBe(200);
    expect(json.suggested_steps).toEqual([{ step_id: 'step-2' }]);
  });

  it('postCopilot surfaces a non-2xx reply as a non-throwing status/json pair', async () => {
    const fetchImpl = vi.fn(
      async () =>
        new Response(JSON.stringify({ error: 'model gateway unavailable' }), {
          status: 502,
          headers: { 'Content-Type': 'application/json' },
        }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postCopilot('run-1', {
      instruction: 'x',
    });
    expect(status).toBe(502);
    expect(json.error).toContain('unavailable');
  });

  it('postCopilot defaults json to {} when a non-2xx reply has an empty body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 500 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postCopilot('run-1', {});
    expect(status).toBe(500);
    expect(json).toEqual({});
  });

  it('postCopilot throws on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.postCopilot('  ', {})).rejects.toThrow(HostClientError);
  });

  it('postCopilot rethrows a genuine network failure instead of swallowing it', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('offline');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(client.postCopilot('run-1', {})).rejects.toThrow(
      /host request failed/,
    );
  });

  it('listActiveHooks defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(await client.listActiveHooks()).toEqual({});
  });

  it('postCopilot defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { json } = await client.postCopilot('run-1', { instruction: 'x' });
    expect(json).toEqual({});
  });

  it('listRuns defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(await client.listRuns()).toEqual({});
  });

  it('getHook defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(await client.getHook('tok-1')).toEqual({});
  });

  it('resumeHook defaults json to {} when the host returns an empty 2xx body', async () => {
    const approval = await issueApprovalRecord({
      proposalDigest: 'sha256:abc',
      tenantId: 'tenant-a',
      principalRef: 'reviewer@local',
      approved: true,
    });
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    expect(await client.resumeHook('tok-1', approval)).toEqual({});
  });

  it('getRecoveryFailure defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.getRecoveryFailure('run-1');
    expect(status).toBe(200);
    expect(json).toEqual({});
  });

  it('postResumeAfterFailure defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.postResumeAfterFailure('run-1', {});
    expect(status).toBe(200);
    expect(json).toEqual({});
  });

  it('startRun defaults json to {} when the host returns an empty 2xx body', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 200 }));
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const { status, json } = await client.startRun('run-1', {
      task_text: 'x',
      permission_ceiling: ['read'],
      approval_token: 'tok',
    });
    expect(status).toBe(200);
    expect(json).toEqual({});
  });

  it('canonicalJson serializes arrays with each element canonicalized', () => {
    expect(canonicalJson([{ z: 1, a: 2 }, 'x', null])).toBe(
      '[{"a":2,"z":1},"x",null]',
    );
  });

  it('sha256Digest throws when Web Crypto subtle is unavailable', async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, 'crypto');
    Object.defineProperty(globalThis, 'crypto', {
      configurable: true,
      value: { ...globalThis.crypto, subtle: undefined },
    });
    try {
      await expect(sha256Digest({ a: 1 })).rejects.toThrow(
        /Web Crypto subtle is unavailable/,
      );
    } finally {
      Object.defineProperty(globalThis, 'crypto', original!);
    }
  });

  it('lists active hooks through GET /v1/hooks/active', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(JSON.stringify({ hooks: [{ token: 'tok-1' }] }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const result = await client.listActiveHooks();
    expect(result.hooks).toEqual([{ token: 'tok-1' }]);
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/hooks/active',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('getRecoveryFailure throws on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.getRecoveryFailure('  ')).rejects.toThrow(HostClientError);
  });

  it('getRecoveryFailure rethrows a genuine network failure instead of swallowing it', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('offline');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(client.getRecoveryFailure('run-1')).rejects.toThrow(
      /host request failed/,
    );
  });

  it('postResumeAfterFailure throws on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.postResumeAfterFailure('  ', {})).rejects.toThrow(
      HostClientError,
    );
  });

  it('postResumeAfterFailure rethrows a genuine network failure instead of swallowing it', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('offline');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(
      client.postResumeAfterFailure('run-1', {}),
    ).rejects.toThrow(/host request failed/);
  });

  it('startRun throws on an empty runId', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(
      client.startRun('  ', {
        task_text: 'x',
        permission_ceiling: ['read'],
        approval_token: 'tok',
      }),
    ).rejects.toThrow(HostClientError);
  });

  it('startRun rethrows a genuine network failure instead of swallowing it', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new TypeError('offline');
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    await expect(
      client.startRun('run-1', {
        task_text: 'x',
        permission_ceiling: ['read'],
        approval_token: 'tok',
      }),
    ).rejects.toThrow(/host request failed/);
  });

  it('gets a hook by token through GET /v1/hooks/{token}', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(
        JSON.stringify({ token: 'tok-1', kind: 'human_approval' }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const result = await client.getHook('tok-1');
    expect(result.kind).toBe('human_approval');
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:9/v1/hooks/tok-1',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('getHook throws on an empty token', async () => {
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.getHook('  ')).rejects.toThrow(HostClientError);
  });

  it('resumes a hook through POST /v1/hooks/{token}/resume', async () => {
    const approval = await issueApprovalRecord({
      proposalDigest: 'sha256:abc',
      tenantId: 'tenant-a',
      principalRef: 'reviewer@local',
      approved: true,
    });
    const fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://127.0.0.1:9/v1/hooks/tok-1/resume');
      expect(JSON.parse(String(init?.body))).toEqual(approval);
      return new Response(JSON.stringify({ status: 'resumed' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
    const result = await client.resumeHook('tok-1', approval);
    expect(result.status).toBe('resumed');
  });

  it('resumeHook throws on an empty token', async () => {
    const approval = await issueApprovalRecord({
      proposalDigest: 'sha256:abc',
      tenantId: 'tenant-a',
      principalRef: 'reviewer@local',
      approved: true,
    });
    const client = new FlowHostClient({
      baseUrl: 'http://127.0.0.1:9',
      fetchImpl: vi.fn() as unknown as typeof fetch,
    });
    await expect(client.resumeHook('  ', approval)).rejects.toThrow(
      HostClientError,
    );
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
