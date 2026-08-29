#ifndef CommunityAIBridge_h
#define CommunityAIBridge_h

#include <stdbool.h>
#include <stdint.h>

char* community_ai_generate_identity(void);
char* community_ai_init_p2p_swarm(const char* node_name);
char* community_ai_tick_governor(bool is_busy, bool on_battery);
char* community_ai_plan_pipeline(const char* model_id, uintptr_t model_size_mb, uint32_t total_layers, const char* nodes_json);
char* community_ai_model_manifest(const char* model_id, uint32_t total_layers, uint32_t num_shards);
void community_ai_free_string(char* s);

#endif /* CommunityAIBridge_h */
