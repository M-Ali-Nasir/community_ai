package ai.community.worker

/**
 * Android JNI / Native Bridge for Community AI True P2P Mesh Engine.
 * Loads libcommunity_ffi.so compiled via cargo ndk.
 */
object CommunityAINative {
    init {
        try {
            System.loadLibrary("community_ffi")
        } catch (e: UnsatisfiedLinkError) {
            // Fallback for emulator / pure web mode
        }
    }

    external fun generateIdentity(): String
    external fun initP2PSwarm(nodeName: String): String
    external fun tickGovernor(isBusy: Boolean, onBattery: Boolean): String
    external fun planPipeline(modelId: String, modelSizeMb: Long, totalLayers: Int, nodesJson: String): String
    external fun modelManifest(modelId: String, totalLayers: Int, numShards: Int): String
}
