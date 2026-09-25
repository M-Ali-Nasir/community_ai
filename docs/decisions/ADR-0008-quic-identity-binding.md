# ADR-0008 — QUIC TLS vs Ed25519 identity binding

**Status:** ACCEPTED  
**Date:** 2026-09-25  
**Supersedes pending note in DECISIONS.md**

## Problem

QUIC requires TLS 1.3. rustls/`quinn` expect X.509 certificates (typically ECDSA/RSA). Community AI identity is Ed25519 (`NodeIdentity`). Binding those two without a custom PKI is non-trivial.

## Options

1. Ed25519 certificates in TLS — poor rustls support; high implementation risk.
2. Pin TLS cert hash into Ed25519-signed Hello — extra coupling; cert rotation pain.
3. **Ephemeral self-signed TLS for confidentiality + application-layer Ed25519 handshake for identity.**

## Decision

Option 3.

- QUIC TLS uses a locally generated self-signed certificate. Clients skip WebPKI verification (`ServerCertVerifier` assertion) **only** because identity is established next.
- After the QUIC connection opens, peers MUST complete `Hello` + `AuthChallenge`/`AuthResponse` signed by the persistent Ed25519 key.
- A connection that fails handshake never becomes `Ready`.
- NodeId is `node-{ed25519_public_key_hex}`.

## Why

Matches ADR-0002 (QUIC data plane) and the existing security crate. Avoids pretending a self-signed TLS name is a peer identity.

## Security considerations

- Confidentiality: QUIC TLS (even with skipped WebPKI) still encrypts the datagrams.
- Authenticity: Ed25519 only.
- MITM on the TLS layer cannot complete AuthResponse without the seed.
- Replay: unique nonces per handshake; envelope signatures include `msg_id` and timestamp.

## Testing

Invalid signing key, protocol version mismatch, and malformed frames must not yield `Ready`.
