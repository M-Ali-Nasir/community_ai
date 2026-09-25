# Agent operating notes

## Roles (this mission)

| Role | Authority |
|------|-----------|
| Manager | Task board, status, blockers, release gates |
| Senior Engineer | Architecture + ADRs; veto incompatible designs |
| P2P Engineer | Transport, discovery, peer FSM, protocol wire |
| AI Engineer | llama.cpp, models, chat path truthfulness |
| Native Engineer | Tauri, Android/iOS FFI, services, packaging |
| QA / Release | Real multi-process tests, build matrices |

## Communication

Write decisions to `docs/decisions/`.  
Write status to `docs/PROJECT_STATUS.md`.  
Do not invent parallel architectures in code without an ADR.

## First coding wave (only after Manager opens READY tasks)

Order: T-010 → T-020 → T-021 → T-022 → T-024 → T-040 → T-042 → T-060.

UI peer cosmetics (T-026) **after** T-021.
