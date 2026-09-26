# BLOCKERS

**Updated:** 2026-09-26  
**Owner:** Manager Agent

| ID | Blocker | Blocks | Severity | Mitigation |
|----|---------|--------|----------|------------|
| B-001 | Physical two-machine LAN mesh not run | T-022 physical DoD | P2 | Not a WAN phase gate; `MESH_VALIDATION.md` |
| B-003 | Native Tauri **window** BUILD/RUNTIME not verified this gate | T-060 desktop DoD | P1 | Source IMPLEMENTED; run `npx tauri dev` when webkit/CLI exist. Do not stamp BUILD VERIFIED from `cargo test`. |
| B-004 | Android JNI / iOS XCFramework **NOT PHYSICALLY TESTED** | Mobile claims | P0 | Same Rust core; **do not claim mobile support** |
| B-006 | Model catalog / GGUF ID mismatch | Operator confusion | P1 | T-030 |
| B-007 | No TS unit tests | PWA confidence | P2 | PWA is not production mesh |
| B-008 | ~~NAT relay undecided~~ | Closed | — | ADR-0012 + `community-relay` (dumb forwarder) |
| B-009 | ~~No automatic reassign~~ | Closed at process level | — | `run_inference_with_reassign` / native attempt log |
| B-010 | **WAN PHYSICAL TEST — NOT TESTED** | T-119 Stage 1 Internet DoD | P0 | **BLOCKED BY TEST ENVIRONMENT** (two ISPs). Use existing harness; **do not redesign network**. Track B continues. |
| B-011 | NAT matrix (public/NAT/CGNAT) unmeasured | Honest WAN NAT claims | P1 | Operator table in `WAN_VALIDATION.md` |
| B-012 | Live TOKEN_STREAM not yet in Tauri UI | T-041 / T-043 complete chat UX | P1 | Core streams on QUIC; IPC returns completed result. Do not fake streaming. |

**Rule:** Do not close a blocker by changing marketing copy alone. Do not mark physical WAN complete without Internet evidence. Do not expand networking solely because B-010 is open.
