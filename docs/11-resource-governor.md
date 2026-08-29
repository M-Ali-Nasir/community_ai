# 11. Resource Governor & User Experience Preservation (UEPS)

## User Experience Preservation Score (UEPS)
$$\text{UEPS} = 1.0 - \left( 0.5 \times \frac{\text{CPU Usage}}{100.0} + 0.5 \times \text{Memory Contention} \right)$$

## State Transition Machine
- `Idle`: User inactive > 2 minutes $\rightarrow$ 85% capacity.
- `Light`: User active recently $\rightarrow$ 50% capacity.
- `Active`: Interactive user applications running $\rightarrow$ 20% capacity.
- `Busy / Gaming`: Heavy foreground task or battery discharge $\rightarrow$ 0% capacity (Immediate VRAM/RAM release).
