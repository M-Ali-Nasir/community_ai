import Foundation

/// Swift wrapper for Community AI Shared Rust Core FFI.
public final class CommunityAIBridge {
    public static let shared = CommunityAIBridge()
    private init() {}

    public func generateIdentity() -> String {
        guard let cStr = community_ai_generate_identity() else { return "" }
        defer { community_ai_free_string(cStr) }
        return String(cString: cStr)
    }

    public func tickGovernor(isBusy: Bool, onBattery: Bool) -> String {
        guard let cStr = community_ai_tick_governor(isBusy, onBattery) else { return "{}" }
        defer { community_ai_free_string(cStr) }
        return String(cString: cStr)
    }

    public func planPipeline(modelId: String, modelSizeMb: Int, totalLayers: UInt32, nodesJson: String) -> String {
        guard let cStr = community_ai_plan_pipeline(modelId, UInt(modelSizeMb), totalLayers, nodesJson) else { return "{}" }
        defer { community_ai_free_string(cStr) }
        return String(cString: cStr)
    }
}
