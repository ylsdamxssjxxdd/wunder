---
title: Monitoring and WunderBench
summary: Observability covers distinct jobs: session monitoring watches threads, tool statistics watch hotspots, performance sampling watches the pipeline, and WunderBench watches model quality.
updated_at: 2026-10-13
---

Observability is split into five areas, each watching something different: whether a thread is stuck, whether a tool is misbehaving, whether the pipeline has slowed down, how speed changes as context grows, and whether model quality has regressed. Most problems need two or three of these together: session monitoring pins down the thread or tool at fault, metrics and logs confirm the scope, and WunderBench shows whether the model side changed.

## Observability Capabilities at a Glance

| Capability | What it watches | Main use |
| --- | --- | --- |
| **Session monitoring** | Thread runtime details | Locate thread state, stalls, and anomalies |
| **Tool statistics** | Tool usage | Find the busiest or least stable tools |
| **Performance sampling** | The system pipeline baseline | Judge capacity and latency bottlenecks |
| **Throughput stress testing** | Model performance across length combinations | Watch how speed changes as context grows |
| **WunderBench** | Task completion quality | Assess how a model performs in wunder and spot regressions |

## Session Monitoring

What it shows:

- The list of active threads
- The current state and stage of each thread
- Token usage statistics
- Tool-call records
- The event timeline

Use it to investigate stuck or abnormal threads, look at the execution detail of a session, force-cancel an abnormal thread, and trigger context compression.

## Tool Statistics

What it shows:

- Call counts per tool
- The tool-call success rate
- Which users use which tools
- How tool calls relate to thread state

Turn to it when a tool has become a performance bottleneck, when calls are unusually frequent, or when you need to assess tool usage patterns.

## Performance and Throughput

### Performance Sampling

Measures the latency baseline of a single request:

- Model response speed
- Tool execution speed
- End-to-end latency

### Throughput Stress Testing

Each run tests one model, one concurrency list, and one input-length and output-length combination directly against the model API, stepping through the list in order; each item fires that many requests in parallel and reports first-token latency, generation speed, and actual usage at each concurrency level.

Input and output keep common presets while also accepting free integer entry within a safe range. The default concurrency list is `1,2,4,8`, the default input is `8.2k` tokens, and the default output is `128` tokens. On the page, token counts show as `k` from 1,000 up and as `m` from 100,000 up. Only supported engines accept fixed-length control parameters; other endpoints get an upper limit and are checked for whether the target was met.

Choosing a virtual model lets you validate the measurement flow. Under Model Configuration → Simulation speed, pick Fast, Medium, or Slow, with Fast as the default: prefill runs at 2000 / 500 / 100 tokens/s, and reasoning and generation run at 200 / 50 / 10 tokens/s. A throughput simulation reasons first and generates content next; reasoning takes a quarter of the requested output total by default and can be adjusted with the simulation reasoning switch and the output budget, and it is listed separately in the details. Details are marked as simulated and tagged with the speed, and history curves are grouped by preset. First-token latency includes the prefill wait; a Fast run with 1m input takes about 524 seconds, so raise the model timeout accordingly or stop it while waiting. A simulation runs the same maximum-context and maximum-output checks, and anything over the limit is recorded as a failure; the default context is 128k, so set the window large enough to hold input plus output before testing 1m.

A test does not create thread logs, and only the 50 most recent summaries are kept. Details and curves show the overall prefill and generation speed along with a separate single-prefill speed and single-generation speed; speeds use `k tok/s` from 1,000 up and `m tok/s` from 100,000 up. Selecting history rows plots the speed curve against the concurrency axis; the vertical axis uses concurrency 1 within the same test as a 0% baseline and shows the percentage change at later concurrency levels, with the measured speed on hover. Results are not additionally flagged as met or not met.

### How the Two Differ

- **Performance sampling**: latency at the single-request level
- **Throughput stress testing**: model API performance across different context and output lengths

## WunderBench Model Evaluation

What it evaluates:

- Task completion quality scores
- Tool-call success rate, stability, efficiency, and context consumption
- Capability comparisons between models
- Regression detection between versions
- Full-bank evaluation, or manual re-runs filtered by task group or task ID

### How It Divides Work with Throughput Testing

- **Throughput stress testing**: focuses on response and generation speed across length combinations
- **WunderBench**: focuses on whether tasks can be completed in the real agent pipeline, and whether quality has regressed

Use WunderBench to assess a new release, compare models, verify that an optimization worked, and inform the default model, the judge model, and tool-enablement policy.

### Output and Export

Evaluation results give:

- A usability verdict in one of four bands: production_ready, usable, risky, not_ready
- The mean task score
- A pass-rate-oriented reliability score
- The tool-result success rate
- A stability score built from completion rate and round-to-round variance
- The task groups most in need of review, and the failed items

There are two ways to export an evaluation record: click **Export evaluation record** on the WunderBench page, or call `GET /wunder/admin/wunderbench/runs/{run_id}/export`.

The exported JSON contains run information, task aggregates, per-attempt detail, task specs, artifact summaries, conversation records, and the persisted monitoring logs for each model attempt. Newly started WunderBench runs record evaluation threads with admin debug logging, so the export can be used to review model requests, model output, tool calls, tool results, workspace updates, and token and speed statistics.

For detailed usage, scoring rules, and extending the task bank, see [WunderBench Model Evaluation](/docs/en/ops/wunderbench/).

## Key Metrics

### Health Metrics

| Metric | Normal range | Alert threshold |
| --- | --- | --- |
| Active sessions | Below 80% of the maximum | Above 80% |
| Queue depth | Below 50 | Above 100 |
| Error rate | Below 1% | Above 5% |
| Average latency | Below 10s | Above 30s |

### Resource Metrics

| Metric | What to watch |
| --- | --- |
| CPU usage | Whether there are abnormal spikes |
| Memory usage | Whether it keeps growing, which may mean a leak |
| Disk usage | Whether workspaces and logs have grown too large |
| Network traffic | Whether there is abnormal traffic |

### Business Metrics

| Metric | What to watch |
| --- | --- |
| Daily active users | The system usage trend |
| Daily token consumption | The resource usage trend |
| Tool-call distribution | How users use tools |
| Channel message volume | Usage across channels |

## Hull Logs

In Hull (server) mode, runtime logs are stored under `config/data/logs/server/`:

- Daily-rotated JSONL files
- Retained for 14 days by default
- Records of requests, tool calls, errors, and more
- When something goes wrong, check the latest logs first

## Routine Checks and Evaluation

### Every Day

- System health
- The error-rate trend
- Queue backlog
- Disk space

### Weekly or Monthly

- The token consumption trend
- Tool usage statistics
- Performance baseline changes
- WunderBench results

## Troubleshooting Order

When a problem comes up, work through this order:

1. Use session monitoring to find the abnormal thread
2. Look at the tool-call records
3. Check the performance metrics
4. Read the Hull logs

## Further Reading

- [Observability](/docs/en/concepts/core-observability/) — the design principles behind observability
- [WunderBench Model Evaluation](/docs/en/ops/wunderbench/) — full-bank evaluation, exports, and extending the task bank
- [Session State Reference](/docs/en/reference/stream-events/) — what the state metrics mean
- [Admin Panels Index](/docs/en/reference/admin-panels/) — where the monitoring entry points are
