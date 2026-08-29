# 09. Model Sharding & Placement Formulation

## Sharding Strategy
Flagship models (such as `Qwen3-14B` with 48 transformer blocks) are divided into discrete blocks:
- **Shard 000**: Embedding table & RMSNorm (Layers 0..6)
- **Shard 001**: Transformer blocks (Layers 7..13)
- **Shard 002**: Transformer blocks (Layers 14..20)
- **Shard 003**: Transformer blocks (Layers 21..27) & LM Head

## Placement Scoring Equation
When a new node $N_j$ connects to the network, candidate shard $S_k$ is selected via:

$$\text{Score}(S_k, N_j) = \frac{\text{Demand}(S_k)}{\text{Replicas}(S_k) + 1} \times \frac{\text{AvailableMemory}(N_j)}{\text{Size}(S_k)} \times \frac{100.0}{\text{AvgRTT}(N_j) + 10.0}$$

This guarantees uniform shard replication across the cluster while ensuring memory fit and low network latency.
