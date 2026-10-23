# CesiumRS token usage

As of 2026-10-23.

Two sessions are recorded so far: 238,082,788 tokens across 666 model replies, about $89.93 at Anthropic's API list prices. Each further session or model adds one row to the ledger, and the totals are recomputed from the rows.

## How to read and extend this ledger

- **What is counted:** the usage Claude's API reports on every reply. That is four kinds of tokens:
  - uncached input;
  - cache writes (context stored for reuse);
  - cache reads (stored context sent again);
  - output (replies, code and tool calls).
- **Why cache reads dominate:** every reply re-sends the whole conversation as context. Cache reads are billed at 0.05x the input rate on Opus 5.5, so they are 98% of the tokens but 52% of the estimated cost.
- **Where the numbers come from:** a session's own transcript, summed with the script under Method. Billing pages or other tools can stand in when no transcript is left; say which in the row's Source column.
- **To add a session:**
  1. Measure it the same way, or copy its figures from your usage page.
  2. Add a row to the ledger.
  3. Price it at its model's rates, adding a Rates row for a new model. Then add its tokens and cost to Running totals.

## Rates

Costs are estimated at Anthropic's API list prices, in USD per million tokens, as read on 2026-10-23 from the [pricing page](https://platform.claude.com/docs/en/about-claude/pricing).

| Model | Input | Cache write, 5 min | Cache write, 1 hour | Cache read | Output |
| --- | --- | --- | --- | --- | --- |
| claude-opus-5-5 | 4.00 | 5.00 | 8.00 | 0.20 | 20.00 |

- **Assumptions:** the estimate uses standard tier with no fast mode, no batch discount and no US-only inference surcharge (1.1x). Every cache write in both recorded sessions used the 1-hour lifetime.
- **Plans:** on a Claude subscription, usage counts against plan limits instead of being billed per token. The figures here are what the same usage would cost on the API, not an invoice.

## Running totals

All recorded sessions together: 238,082,788 tokens, about $89.93 at API list prices. Cache reads are 98.3% of the tokens but 52.0% of the cost.

| Token kind | Tokens | Share of tokens | Est. cost (USD) | Share of cost |
| --- | --- | --- | --- | --- |
| Cache reads | 234,013,438 | 98.29% | 46.80 | 52.0% |
| Cache writes (1 hour) | 3,186,731 | 1.34% | 25.49 | 28.3% |
| Output | 881,271 | 0.37% | 17.63 | 19.6% |
| Uncached input | 1,348 | 0.00% | 0.01 | 0.0% |
| **Total** | **238,082,788** | **100%** | **89.93** | **100%** |

By model:

| Model | Sessions | Replies | Output | Cache writes | Cache reads | Uncached input | Total | Est. cost (USD) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| claude-opus-5-5 | 2 | 666 | 881,271 | 3,186,731 | 234,013,438 | 1,348 | 238,082,788 | 89.93 |

## Ledger

One row per session, newest first. Of the sessions before PR #41, only the PR #40 shader session has a row; the others ran in other containers and still need rows.

| Session | Surface | Model | Start (UTC) | End (UTC) | Work covered | Replies | Output | Cache writes | Cache reads | Uncached input | Total | Est. cost (USD) | Source |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `session_01QjMqDT4irwUeBX5A32h8m5` | Claude Code, cloud | claude-opus-5-5 | 2026-10-22 07:42 | 2026-10-23 01:02 | PR #40: all 307 Source/Shaders modules | 70 | 48,714 | 295,174 | 9,489,073 | 140 | 9,833,101 | 5.23 | Session transcript |
| `session_014SmZF66c7LJ7rqzaz1c16Q` | Claude Code, cloud | claude-opus-5-5 | 2026-10-21 21:59 | 2026-10-23 00:58 | PRs #41–#44: workers to 100%, Scene foundations, GltfPipeline, 3D Metadata | 596 | 832,557 | 2,891,557 | 224,524,365 | 1,208 | 228,249,687 | 84.69 | Session transcript |

## Session detail: Oct 22–23, 2026

The cloud session `session_01QjMqDT4irwUeBX5A32h8m5` used 80% of its tokens on Oct 22, the day it ported all 307 shader modules and put up PR #40.

| Day (UTC) | Replies | Output | Cache writes | Cache reads | Uncached input | Total | Est. cost (USD) | Work |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-10-23 | 10 | 2,733 | 167,521 | 1,762,315 | 20 | 1,932,589 | 1.75 | This ledger entry |
| 2026-10-22 | 60 | 45,981 | 127,653 | 7,726,758 | 120 | 7,900,512 | 3.49 | Builtin, Appearances and Materials shaders; root, Model, PostProcessStages and Voxels shaders; PR #40 description |

- **Cache writes on Oct 23:** the session resumed after about 17 hours idle, past the 1-hour cache lifetime, so its whole context was written to the cache again.
- **Rounding:** the day costs add up to $5.24, a cent above the ledger's $5.23, because each day is rounded on its own.
- **Subagents:** none ran; every reply came from the main conversation.
- **Snapshot:** the counts were taken at 2026-10-23 01:02 UTC. Replies after that, including the rest of this entry's writing, are not in them.

## Session detail: Oct 21–23, 2026

The cloud session `session_014SmZF66c7LJ7rqzaz1c16Q` used 63% of its tokens on Oct 22, the day of the Scene foundations and GltfPipeline batches.

| Day (UTC) | Replies | Output | Cache writes | Cache reads | Uncached input | Total | Est. cost (USD) | Work |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-10-23 | 91 | 124,403 | 354,853 | 29,186,310 | 188 | 29,665,754 | 11.16 | 3D Metadata entities, PR #44 CI fix, this ledger |
| 2026-10-22 | 339 | 531,158 | 1,934,442 | 140,429,434 | 686 | 142,895,720 | 54.19 | Scene foundations, GltfPipeline (PR #42), 3D Metadata schema (PR #43) and tables |
| 2026-10-21 | 167 | 176,996 | 602,262 | 54,908,621 | 334 | 55,688,213 | 19.34 | Remaining worker modules and upstream sync (PR #41) |

- **Replies:** the day rows add up to 597. One is a synthetic reply with zero usage, which the 596 in the ledger leaves out.
- **Subagents:** none ran; every reply came from the main conversation.
- **Snapshot:** the counts were taken at 2026-10-23 00:58 UTC. Replies after that, including the rest of this ledger's writing, are not in them.

## Method

Run this on a Claude Code transcript (`~/.claude/projects/<project>/<session>.jsonl`) to get one ledger row's figures. It counts each reply once by message id, because a reply's usage can be logged more than once while it streams. It prints totals by model and by day.

```python
import json, sys, collections
KINDS = ['input_tokens', 'output_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens']
seen = set()
by_model = collections.defaultdict(collections.Counter)
by_day = collections.defaultdict(collections.Counter)
for line in open(sys.argv[1]):
    try:
        d = json.loads(line)
    except ValueError:
        continue
    m = d.get('message') or {}
    usage = m.get('usage')
    if d.get('type') != 'assistant' or not usage:
        continue
    key = m.get('id') or d.get('uuid')
    if key in seen:
        continue
    seen.add(key)
    model = m.get('model', '?')
    day = (d.get('timestamp') or '')[:10]
    for k in KINDS:
        by_model[model][k] += usage.get(k) or 0
        by_day[day][k] += usage.get(k) or 0
    by_model[model]['replies'] += 1
    by_day[day]['replies'] += 1
for name, table in (('model', by_model), ('day', by_day)):
    for key, c in sorted(table.items()):
        print(name, key, dict(c), 'total', sum(c[k] for k in KINDS))
```

For other tools or the API, the same four fields appear in each response's `usage` object. Add them per reply and record the surface in the ledger's Source column.

Cost per kind = tokens ÷ 1,000,000 × the model's rate from Rates. Cache writes take the rate for their lifetime; a transcript's `usage.cache_creation` splits them into 5-minute and 1-hour writes.
