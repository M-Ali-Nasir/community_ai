//! Distributed Cluster Scheduler & Pipeline Planner.
//! Decides optimal placement (Single-Node vs. Model-Parallel vs. Task-Parallel)
//! and assembles minimal-latency pipeline chains.

use community_core::{CommunityError, Result};
use community_protocol::{
    CapabilityProfile, DistributionStrategy, JobRequest, PipelineMember, PipelinePlan,
};

pub struct Scheduler;

impl Scheduler {
    /// Analyzes the job request and cluster nodes to pick the optimal execution strategy.
    pub fn analyze_workload(
        _request: &JobRequest,
        model_size_mb: usize,
        nodes: &[CapabilityProfile],
    ) -> DistributionStrategy {
        // Check if any single node can hold the entire model alone
        let single_fits = nodes
            .iter()
            .any(|n| n.memory.available_mb >= (model_size_mb as f32 * 1.15) as usize);

        if single_fits {
            DistributionStrategy::SingleNode
        } else {
            // If pooled memory fits, form a layer-split pipeline
            let total_pooled: usize = nodes.iter().map(|n| n.memory.available_mb).sum();
            if total_pooled >= (model_size_mb as f32 * 1.15) as usize {
                DistributionStrategy::ModelParallel
            } else {
                DistributionStrategy::SingleNode
            }
        }
    }

    /// Assembles an optimal pipeline plan minimizing token hop RTT.
    pub fn plan_pipeline(
        model_id: &str,
        model_size_mb: usize,
        total_layers: u32,
        nodes: &[CapabilityProfile],
    ) -> Result<PipelinePlan> {
        let needed_memory = (model_size_mb as f32 * 1.15) as usize;
        let mut candidates: Vec<&CapabilityProfile> = nodes
            .iter()
            .filter(|n| n.memory.available_mb >= 256)
            .collect();

        if candidates.is_empty() {
            return Err(CommunityError::ResourceExhausted("No nodes available for pipeline".into()));
        }

        // Sort candidates by lowest network latency first
        candidates.sort_by(|a, b| {
            a.network
                .latency_ms
                .partial_cmp(&b.network.latency_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut chosen = Vec::new();
        let mut accumulated_memory = 0;

        for c in candidates {
            chosen.push(c);
            accumulated_memory += c.memory.available_mb;
            if accumulated_memory >= needed_memory {
                break;
            }
        }

        if accumulated_memory < needed_memory {
            return Err(CommunityError::ResourceExhausted(format!(
                "Pooled memory ({accumulated_memory}MB) is less than required ({needed_memory}MB)"
            )));
        }

        let total_members = chosen.len() as u32;
        let layers_per_node = (total_layers + total_members - 1) / total_members;

        let members: Vec<PipelineMember> = chosen
            .iter()
            .enumerate()
            .map(|(idx, n)| {
                let start = (idx as u32) * layers_per_node;
                let end = ((idx as u32 + 1) * layers_per_node).min(total_layers);
                let share = (end - start) as f32 / (total_layers as f32);
                PipelineMember {
                    node_id: n.node_id.clone(),
                    label: n.label.clone(),
                    endpoint: n
                        .rpc
                        .as_ref()
                        .and_then(|r| r.endpoint.clone())
                        .unwrap_or_else(|| format!("{}:50052", n.label)),
                    layer_start: start,
                    layer_end: end,
                    share,
                    assigned_memory_mb: (model_size_mb as f32 * share) as usize,
                }
            })
            .collect();

        let hop_rtt_sum: f32 = chosen.iter().map(|n| n.network.latency_ms).sum();
        let latency_ceiling = if hop_rtt_sum > 0.0 {
            1000.0 / hop_rtt_sum
        } else {
            100.0
        };

        Ok(PipelinePlan {
            model_id: model_id.to_string(),
            head_node_id: chosen[0].node_id.clone(),
            members,
            pooled_memory_mb: accumulated_memory,
            estimated_hop_rtt_ms: hop_rtt_sum,
            latency_ceiling_tok_per_sec: latency_ceiling,
            reason: format!(
                "Formed {total_members}-node pipeline with {accumulated_memory}MB pooled memory"
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use community_core::NodeId;
    use community_protocol::*;

    fn mock_node(name: &str, mem_mb: usize, rtt_ms: f32) -> CapabilityProfile {
        CapabilityProfile {
            node_id: NodeId::new(name),
            label: name.to_string(),
            kind: NodeKind::DesktopWorker,
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu: CpuProfile {
                model: "AMD".into(),
                cores: 8,
                available_fraction: 0.8,
            },
            gpu: None,
            memory: MemoryProfile {
                total_mb: mem_mb * 2,
                available_mb: mem_mb,
            },
            network: NetworkProfile {
                latency_ms: rtt_ms,
                bandwidth_mbps: 100.0,
                jitter_ms: 2.0,
            },
            user_state: UserState {
                activity: UserActivity::Idle,
                thermal_state: ThermalState::Normal,
                on_battery: false,
                battery_pct: None,
            },
            rpc: Some(RpcProfile {
                endpoint: Some(format!("{name}:50052")),
                offered_memory_mb: mem_mb,
                can_head: true,
                build_tag: "b10632".into(),
            }),
            cached_shards: vec![],
        }
    }

    #[test]
    fn test_pipeline_planner() {
        let nodes = vec![
            mock_node("node-fast-small", 2000, 5.0),
            mock_node("node-med-small", 2000, 15.0),
            mock_node("node-slow-big", 8000, 60.0),
        ];

        // 3000MB model with 15% headroom requires 3450MB (fits in node1 + node2 = 4000MB)
        let plan = Scheduler::plan_pipeline("qwen2.5-7b", 3000, 28, &nodes).unwrap();
        assert_eq!(plan.members.len(), 2);
        assert_eq!(plan.head_node_id.as_str(), nodes[0].node_id.as_str());
    }
}
