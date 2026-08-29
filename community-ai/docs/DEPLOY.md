# Running a testnet with friends

Target: one coordinator and 10–20 heterogeneous personal computers.

The advice here is deliberately boring. The interesting part of this project is
the scheduler, not the network plumbing, and every hour spent on NAT traversal
is an hour not spent on the actual question.

## Do not expose the coordinator to the public internet

Not yet. Use a private overlay network instead. You get private addressing,
encrypted transport, no port forwarding, no NAT holes, and stable IPs that
survive everyone's router rebooting.

[Tailscale](https://tailscale.com) is free for personal use at this scale and
takes about five minutes. [ZeroTier](https://zerotier.com) works the same way if
you prefer it.

### Coordinator machine

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up

tailscale ip -4        # e.g. 100.101.102.103
```

Then start it:

```bash
git clone <your repo> && cd community-ai
npm install
npm run build

JOIN_TOKEN=$(openssl rand -hex 16) npm start
```

Write the token down; everyone needs it. The startup banner marks which of its
addresses is on the overlay network:

```
  Community AI coordinator
  ------------------------
  http://localhost:8787
  http://192.168.1.9:8787
  http://100.101.102.103:8787   <- overlay network

  join token   : required
```

### Each friend's machine

Install Tailscale, accept your invite, then:

```bash
git clone <your repo> && cd community-ai
npm install
npm run build

node packages/worker-node/dist/index.js \
  --coordinator http://100.101.102.103:8787 \
  --token <the join token> \
  --name "toms-gaming-pc" \
  --model qwen2.5-1.5b
```

The first run downloads the model once into `./models`. Subsequent runs start in
seconds.

### Phones

Open `http://100.101.102.103:8787?token=<join token>` in the Tailscale-connected
browser and install it to the home screen. The token is consumed from the URL
and stored, so it does not linger in history.

Phones are chat clients by default. Contributing is opt-in, on the Contribute
tab, and clearly labelled as experimental.

## What to tell people to expect

Be honest about it up front, or you will spend the whole experiment answering
the same three questions.

- The worker uses **spare** capacity. It backs off within a second or two of you
  starting to use the machine, and stops entirely if you launch something heavy.
- `p` pauses instantly and abandons whatever was running. `q` quits.
- Laptops on battery do not contribute by default.
- The first run downloads a few hundred MB to a few GB of model weights.
- Nothing about their machine leaves it except the capability profile: CPU
  model, GPU model, free memory, latency, and whether they are idle.

## Sensible per-machine settings

| Machine | Suggested flags |
| --- | --- |
| Desktop with a discrete GPU, usually idle | `--model qwen3-14b --max-capacity 0.9` |
| Desktop that also gets gamed on | `--model qwen2.5-1.5b --pause-cpu 60` |
| Work laptop | `--model qwen2.5-0.5b --max-capacity 0.4 --throttle-cpu 35` |
| Laptop the owner wants to run on battery | `--run-on-battery --battery-capacity 0.3` |
| Mac with Apple Silicon | `--model qwen3-14b` (Metal is detected automatically) |

`--help` lists every governor knob. All of them are enforced on the friend's own
machine; the coordinator cannot raise any of them.

## Verifying the network

```bash
curl -s http://100.101.102.103:8787/api/state | python3 -m json.tool

node scripts/smoke.mjs chat  "Say hello."
node scripts/smoke.mjs batch
```

The batch run is the interesting one: it prints which nodes were selected, the
share each received, and the measured tokens per second per node.

To compare scheduling policies on the same live network:

```bash
for p in adaptive best-node compute-only network-aware round-robin; do
  echo "=== $p"
  POLICY=$p node scripts/smoke.mjs batch 2>&1 | grep -E "wall=|CLUSTER"
done
```

## If you must expose it publicly

Put a TLS reverse proxy in front (Caddy is the least effort), keep `JOIN_TOKEN`
set to something long, and be aware that the token is currently the only
authentication — there is no per-user identity, no rate limiting, and no
sandboxing of the prompts a client can submit. That is acceptable for a testnet
among friends on a private network and not acceptable on the open internet.

```caddyfile
ai.example.com {
  reverse_proxy localhost:8787
}
```

Workers then connect with `--coordinator https://ai.example.com`; the client
upgrades to `wss://` automatically.
