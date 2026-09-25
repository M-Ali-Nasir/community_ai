# WAN Stage 1 validation

**Invariant:** No coordinator, cloud inference, browser, or WebRTC on the inference path.

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

Do **not** mix labels.

| Label | Meaning |
|-------|---------|
| **PROCESS VERIFIED** | Independent native processes (often one host / loopback) |
| **NETWORK EMULATED** | Real stack under `tc netem` / namespaces |
| **PHYSICAL WAN VERIFIED** | Two devices on **different public Internet connections** (not same LAN) |
| **NOT TESTED** | No evidence |

Software **never** stamps `PHYSICAL WAN VERIFIED`. An operator with two ISPs must fill the physical section.

---

## Harness (repeatable)

Worker and originator are **process jobs**, not network ranks.

```bash
# Worker (keep running)
community-daemon --mode worker --name peer-b --port 4433 --no-mdns \
  --model /path/to.gguf --model-id wan-gguf

# Originator (one task, report, exit)
community-daemon --mode originator --name peer-a --port 4434 --no-mdns \
  --peer B.ADDRESS:4433 --model-id wan-gguf \
  --prompt "Reply with one short sentence about rivers." \
  --report-json /tmp/wan-report.json
```

Optional opaque relay (not a coordinator):

```bash
community-relay --bind 0.0.0.0:3478
# then add --relay RELAY.PUBLIC:3478 on both peers
```

Same-host loopback:

```bash
./scripts/wan-inference-harness.sh
# WAN_USE_RELAY=1 ./scripts/wan-inference-harness.sh
```

The loopback harness **must** print `CLASSIFICATION: PROCESS VERIFIED` and must **not** print `PHYSICAL WAN VERIFIED`.

---

## Connection report (every session)

Each Ready peer logs:

```text
Peer ID
Local endpoint
Observed/session endpoint
Advertised listen
Reflexive endpoint
Relay endpoint
Connection mode: DIRECT | RELAY | FAILED
RTT
Evidence class: PROCESS_VERIFIED | LAN_CANDIDATE | WAN_CANDIDATE
```

`RELAY` is never called direct P2P.

---

## Results — this development host (2026-09-25)

Single Linux machine. One ISP. **Cannot** satisfy “two different Internet connections.”

| Scenario | Label | Evidence |
|----------|-------|----------|
| Originator → worker llama.cpp (loopback QUIC) | **PROCESS VERIFIED** | `./scripts/wan-inference-harness.sh` (2026-09-25: DIRECT, 26 llama.cpp tokens, TTFT 128ms) |
| Connection mode DIRECT on loopback | **PROCESS VERIFIED** | `loopback_connection_is_direct_and_process_verified` |
| Opaque UDP relay forwards bytes | **PROCESS VERIFIED** | `std_relay_forwards_opaque_bytes` |
| QUIC session via local `community-relay` | **PROCESS VERIFIED** | `two_peers_quic_through_opaque_relay` (mode=RELAY) |
| Worker kill mid-task | **PROCESS VERIFIED** | `worker_disappear_fails_task` |
| Originator reassign A→C + real llama | **PROCESS VERIFIED** | `reassign_skip_rejecting_peer_then_llama` |
| Invalid identity / signature | **PROCESS VERIFIED** | `invalid_identity_rejected`, `frame_rejects_wrong_signature` |
| Spoofed task origin | **PROCESS VERIFIED** | `spoofed_origin_id_is_rejected` |
| Oversized / malformed task | **PROCESS VERIFIED** | `oversized_and_malformed_tasks_rejected` |
| Stale frame timestamp | **PROCESS VERIFIED** | `stale_timestamp_is_rejected` |
| Live STUN to public server | **NOT TESTED** | Optional infra; decode unit only |
| `tc netem` latency/loss | **NETWORK EMULATED — NOT RUN** | `scripts/mesh-netem-test.sh` (needs root+`tc`) |
| Two machines same LAN | **NOT TESTED** | Not WAN |
| **A ISP ↔ B ISP real tokens** | **NOT TESTED** | Blocker B-010 |

---

## Physical WAN procedure (operator)

Use two **different** ISPs (phone tether + home, office + friend, etc.). Not two Wi-Fi clients on one router.

1. On B (worker): start `community-daemon --mode worker --no-mdns --model …`. Record **reflexive** endpoint from logs (STUN) or a reachable `--bind` address.
2. On A (originator): `--mode originator --no-mdns --peer B_REFLEXIVE_OR_PUBLIC:port`.
3. Confirm originator prints `Connection mode: DIRECT` or `RELAY` **explicitly**.
4. Confirm `engine=llama.cpp`, non-empty tokens, not a joke template.
5. Fill the table below. If STUN/direct fails, retry with `--relay` on a third host that only forwards UDP.

### Physical WAN record

| Date | A network | B network | Mode | RTT | TTFT | tok/s | Result |
|------|-----------|-----------|------|-----|------|-------|--------|
| — | — | — | — | — | — | — | **NOT TESTED** |

Stamp **PHYSICAL WAN VERIFIED** only after a row exists with real ISP names and `completion=ok`.

---

## NAT matrix

Do not require every cell to succeed. Empty cells are **NOT TESTED** unless noted.

| A \ B | Public | NAT | CGNAT |
|-------|--------|-----|-------|
| Public | **NOT TESTED** | **NOT TESTED** | **NOT TESTED** |
| NAT | **NOT TESTED** | **NOT TESTED** | **NOT TESTED** |
| CGNAT | **NOT TESTED** | **NOT TESTED** | **NOT TESTED** |

Loopback / same host is **not** a NAT matrix entry.

Expected (design, not evidence): cone NAT often DIRECT after STUN + simultaneous dial; symmetric/CGNAT often RELAY or FAILED. If a cell stays FAILED without relay, that is a **blocker**, not a pass.

---

## Network conditions

| Test | Label | Status |
|------|-------|--------|
| A normal Internet | PHYSICAL WAN | **NOT TESTED** |
| B higher latency | NETWORK EMULATED | **NOT RUN** (`mesh-netem-test.sh`) |
| C packet loss | NETWORK EMULATED | **NOT RUN** |
| D temporary disconnect | PROCESS VERIFIED | `worker_disappear_fails_task` / `peer_disappear_mesh_survives` |
| E endpoint change, stable identity | PROCESS VERIFIED | `identity_stable_when_listen_port_changes` |

---

## Performance baseline

**PROCESS VERIFIED** loopback (2026-09-25, Qwen2.5 0.5B Q4_K_M, this host):

| Metric | Value |
|--------|-------|
| connection_mode | DIRECT |
| connect_ms | 44 |
| time_to_first_token_ms | 128 |
| total_generation_ms | 844 |
| tokens | 26 |
| tokens_per_sec | 30.81 |
| bytes_approx | 190 |

This is **not** a WAN RTT/throughput baseline.

WAN baseline: **NOT TESTED**.

---

## Security (hostile Internet)

| Attack | Result | Label |
|--------|--------|-------|
| Wrong signature | disconnect, not Ready | PROCESS VERIFIED |
| Pubkey / node id mismatch | rejected | PROCESS VERIFIED |
| Protocol version mismatch | rejected | PROCESS VERIFIED |
| Malformed frame | rejected | PROCESS VERIFIED |
| Stale timestamp | rejected | PROCESS VERIFIED |
| Spoofed `origin_id` | TASK_REJECT | PROCESS VERIFIED |
| Oversized prompt | local reject | PROCESS VERIFIED |
| Template / simulated engine | originator reject | PROCESS VERIFIED |
| Relay as authority | relay cannot parse QUIC | design + opaque forward PROCESS VERIFIED |

---

## Success criterion (not yet met)

```text
Device A  --public Internet-->  Device B (llama.cpp + GGUF)  --> real tokens --> A
```

with no coordinator, no cloud inference, no browser, no WebRTC, no simulation.

**Stage 1 PHYSICAL WAN: NOT COMPLETE.**
