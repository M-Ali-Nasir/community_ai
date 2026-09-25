# BLOCKERS

**Updated:** 2026-09-25  
**Owner:** Manager Agent

| ID | Blocker | Blocks | Severity | Mitigation |
|----|---------|--------|----------|------------|
| B-001 | Physical two-machine LAN mesh not run | T-022 physical DoD | P2 | Not a WAN phase gate; `MESH_VALIDATION.md` |
| B-003 | Native Tauri **window** not built here | Polished desktop UX | P1 | `community-app` API exists; `apps/desktop` scaffold |
| B-004 | Android JNI / iOS XCFramework untested | Mobile | P0 | Same Rust core; **do not claim mobile support** |
| B-006 | Model catalog / GGUF ID mismatch | Operator confusion | P1 | T-030 |
| B-007 | No TS unit tests | PWA confidence | P2 | PWA is not production mesh |
| B-008 | ~~NAT relay undecided~~ | Closed | — | ADR-0012 + `community-relay` (dumb forwarder) |
| B-009 | ~~No automatic reassign~~ | Closed at process level | — | `run_inference_with_reassign` |
| B-010 | **WAN PHYSICAL TEST — NOT TESTED** | T-119 Stage 1 Internet DoD | P0 | Two different ISPs; never fake; `WAN_VALIDATION.md` |
| B-011 | NAT matrix (public/NAT/CGNAT) unmeasured | Honest WAN NAT claims | P1 | Operator table in `WAN_VALIDATION.md`; FAILED cells become concrete blockers |

**Rule:** Do not close a blocker by changing marketing copy alone. Do not mark physical WAN complete without Internet evidence.
