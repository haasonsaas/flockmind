# FlockMind

A distributed hive daemon with an LLM brain. Nodes persist across authorized hosts only - no self-propagation, just resilient distributed coordination with intelligent planning.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         FlockMind Node                          │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │   Brain      │  │  Replicator  │  │     Executor         │  │
│  │  (LLM)       │──│  (Raft)      │──│  (Task Runner)       │  │
│  │              │  │              │  │                      │  │
│  │ Plans actions│  │ Consensus    │  │ Validates & runs     │  │
│  │ from goals   │  │ across nodes │  │ actions locally      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
│                              │                                   │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Attachments Registry                   │  │
│  │   Directories, Files, Services, Docker, Webhooks         │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Components

- **Replicator**: Raft-based consensus (via openraft) for cluster state
- **Brain**: LLM planner (via async-openai) that proposes typed actions
- **Executor**: Validates actions against policy, runs tasks locally
- **Attachments**: Registry of resources the node can interact with

## Safety

- Nodes only join via explicit enrollment (no scanning/propagation)
- LLM outputs constrained to typed `BrainAction` enum
- Policy engine gates all actions (blocked paths, approval requirements)
- No arbitrary shell execution - only predefined task types

## Quick Start

```bash
# Initialize config
./flockmind init

# Edit flockmind.toml to configure

# Run daemon
./flockmind run

# In another terminal, interact via CLI
./flockctl status
./flockctl cluster
./flockctl goal add -d "Keep nginx running on all nodes tagged 'web'"
./flockctl task submit -n node-1 --echo "Hello hive"
```

## Configuration

See `flockmind.example.toml` for full options.

Key settings:
- `tags`: Node labels for placement decisions
- `llm.enabled`: Enable/disable LLM brain
- `llm.model`: OpenAI model to use
- `policy.*`: Execution constraints

## API Endpoints

- `GET /health` - Health check
- `GET /status` - Node status
- `GET /cluster` - Full cluster view
- `GET /tasks` - List tasks
- `POST /tasks` - Submit task
- `GET /goals` - List goals
- `POST /goals` - Add goal
- `GET /attachments` - List attachments

## Task Types

```rust
TaskPayload::Echo { message }
TaskPayload::CheckService { service_name }
TaskPayload::RestartService { service_name }
TaskPayload::SyncDirectory { src, dst }
TaskPayload::DockerRun { image, args }
```

## Brain Actions

The LLM can propose:
- `ScheduleTask` - Run a task on a node
- `RebalanceTask` - Move task to different node
- `CancelTask` - Cancel a task
- `MarkNodeDegraded` - Flag unhealthy node
- `CreateAttachment` / `RemoveAttachment`
- `RequestHumanApproval` - Escalate for approval
- `NoOp` - Do nothing (with reason)

## Multi-Node Setup

1. Start first node, it initializes as single-node cluster
2. Add peer configs to other nodes
3. Future: mTLS enrollment for secure cluster formation

## License

MIT
