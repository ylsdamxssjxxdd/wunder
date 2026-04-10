---
title: System Architecture
summary: wunder carries server, cli, and desktop running modes through a unified scheduling kernel.
read_when:
  - You need to understand the overall structure of wunder
  - You need to understand how APIs, scheduling, tools, frontend, and storage work together.
source_docs:
  - "docs/系统介绍.md"
  - "docs/设计方案.md"
---
# System Architecture
Wunder's architectural goal is very clear: to use a unified core to support multiple operating forms and multiple sources of capability.
## Top-Level Structure
From the warehouse structure and current implementation, wunder can be divided into several layers:
1. Access Layer
2. Scheduling Layer
3. Tools and Capabilities Layer
4. Storage and Workspace Layer
5. Front-End and Desktop Shell Layer
## Access Layer
The access layer mainly includes:
- `/wunder` main interface
- Chat WebSocket and SSE
- User-side frontend
- Admin-side frontend
- Channel entry
- A2A and MCP access points
The function of this layer is to unify 'how the external enters the system'.
## Scheduling Layer
The scheduling layer is the core of the system, mainly composed of these modules:
- `src/api/`
- `src/orchestrator/`
- `src/services/`
- `src/core/`

It is responsible for:
- Parse requests
- Manage sessions and threads
- Build model context
- Initiate model calls
- Handle tool calls
- Log events and states
## Tools and Capability Layer
The ability of wunder does not come from just one place, but from multiple sources unified:
- Built-in Tools
- MCP Tools
- Skills
- Knowledge Base
- User Tools
- Swarm and Multi-Agent Collaboration Capabilities
This is also one of the biggest differences between Wunder and simple chat products.
## Storage and Workspace Layer
Workspaces and storage are the foundation for long-term operations:
- User workspace is used for persistent files and artifacts
- Session data is used for history and playback
- Monitoring and events are used for observability
- Long-term memory is used for memory injection during thread initialization
## Front-end and Desktop Shell
wunder currently has at least three user-visible interfaces:
- User-side frontend: `frontend/`
- Admin-side frontend: `web/`
- Desktop shell: `desktop/`
They share underlying capabilities, but their interaction goals are different.
## The Most Important Current Architectural Constraints
- `server` is the platform core
- `desktop` is the current main delivery form
- `cli` is the development and automation entry
- The user-side frontend and admin-side frontend must be distinguished
- WebSocket is preferred, SSE as a fallback
## Illustration
The current manual retains a core hierarchy diagram, suitable for first establishing the system boundaries:
- [Hierarchy Diagram (Heartship -> User -> Swarm -> Agent -> Thread)](/docs/assets/manual/08-hierarchy-structure.svg)
## Further Reading
- [Conversations and Turns](/docs/en/concepts/sessions-and-rounds/)
- [Tool System](/docs/en/concepts/tools/)
- [Deployment and Operation](/docs/en/ops/deployment/)
